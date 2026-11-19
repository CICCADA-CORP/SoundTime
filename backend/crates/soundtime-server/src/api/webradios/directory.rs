//! Radio Browser catalogue metadata. Media URLs are returned to the player and
//! the directory client never fetches media from the broadcaster.
use axum::http::StatusCode;
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::{OnceCell, Semaphore};
use url::{Host, Url};
use uuid::Uuid;

const MIRRORS: &[&str] = &[
    "https://all.api.radio-browser.info",
    "https://de1.api.radio-browser.info",
];
const MAX_PAGES: usize = 64;
const MAX_PARALLEL: usize = 12;
const PAGE_TTL: Duration = Duration::from_secs(90);
const ERROR_TTL: Duration = Duration::from_secs(2);
const MAX_JSON: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub stations: Vec<Station>,
    pub next_offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Station {
    pub id: Uuid,
    pub name: String,
    pub icon_url: Option<String>,
    pub description: String,
    pub tags: Vec<String>,
    pub country: String,
    pub language: String,
    pub codec: String,
    pub bitrate: u32,
    pub hls: bool,
}

#[derive(Debug, Serialize)]
pub struct Stream {
    pub url: String,
    pub hls: bool,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Listing {
    stationuuid: String,
    name: String,
    url: String,
    url_resolved: String,
    favicon: String,
    description: Option<String>,
    tags: String,
    country: String,
    countrycode: String,
    language: String,
    codec: Option<String>,
    bitrate: u32,
    hls: u8,
    lastcheckok: u8,
}

fn clean(value: &str, limit: usize) -> String {
    value
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}

pub(super) fn web_url(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() > 2048 || value.chars().any(char::is_control) {
        return None;
    }
    let url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let public = match url.host()? {
        Host::Domain(host) => {
            let host = host.trim_end_matches('.');
            host.contains('.')
                && host != "home.arpa"
                && ![".localhost", ".local", ".internal", ".home.arpa"]
                    .iter()
                    .any(|suffix| host.ends_with(suffix))
        }
        Host::Ipv4(ip) => super::relay::public_address(ip.into()),
        Host::Ipv6(ip) => super::relay::public_address(ip.into()),
    };
    public.then(|| url.to_string())
}

impl Listing {
    fn stream(&self) -> Option<Stream> {
        if self.lastcheckok != 1 {
            return None;
        }
        let value = if self.url_resolved.trim().is_empty() {
            &self.url
        } else {
            &self.url_resolved
        };
        Some(Stream {
            url: web_url(value)?,
            hls: self.hls == 1,
        })
    }

    fn station(&self) -> Option<Station> {
        self.stream()?;
        let id = Uuid::parse_str(&self.stationuuid).ok()?;
        let name = clean(&self.name, 256);
        if name.is_empty() {
            return None;
        }
        let country = clean(
            if self.country.trim().is_empty() {
                &self.countrycode
            } else {
                &self.country
            },
            80,
        );
        let language = clean(&self.language, 100);
        let mut codec = clean(self.codec.as_deref().unwrap_or_default(), 24);
        if codec.eq_ignore_ascii_case("unknown") {
            codec.clear();
        }
        let mut description = clean(self.description.as_deref().unwrap_or_default(), 400);
        if description.is_empty() {
            let mut fields: Vec<_> = [&country, &language, &codec]
                .into_iter()
                .filter(|s| !s.is_empty())
                .cloned()
                .collect();
            if self.bitrate > 0 {
                fields.push(format!("{} kbps", self.bitrate));
            }
            description = fields.join(" · ");
        }
        let mut tags = Vec::new();
        let mut seen = HashSet::new();
        for raw in self.tags.split(',') {
            let tag = clean(raw, 64);
            if !tag.is_empty() && seen.insert(tag.to_lowercase()) {
                tags.push(tag);
                if tags.len() == 16 {
                    break;
                }
            }
        }
        Some(Station {
            id,
            name,
            icon_url: web_url(&self.favicon).filter(|u| u.starts_with("https://")),
            description,
            tags,
            country,
            language,
            codec,
            bitrate: self.bitrate,
            hls: self.hls == 1,
        })
    }
}

#[derive(PartialEq, Eq)]
struct Search {
    name: String,
    tag: String,
    offset: u32,
    limit: u32,
}

type CachedPage = Arc<OnceCell<Result<Page, StatusCode>>>;

struct Entry {
    search: Search,
    created: Instant,
    page: CachedPage,
}

pub struct Directory {
    pub(super) podcasts: super::podcasts::Podcasts,
    client: Client,
    mirrors: Vec<String>,
    preferred: AtomicUsize,
    pages: Mutex<VecDeque<Entry>>,
    permits: Semaphore,
}

impl Directory {
    pub fn new() -> anyhow::Result<Self> {
        Self::with_mirrors(MIRRORS.iter().map(|s| s.to_string()).collect())
    }

    fn with_mirrors(mirrors: Vec<String>) -> anyhow::Result<Self> {
        anyhow::ensure!(!mirrors.is_empty(), "Radio Browser requires a mirror");
        let client = Client::builder()
            .user_agent(concat!("SoundTime/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(4))
            .redirect(reqwest::redirect::Policy::none())
            .pool_max_idle_per_host(MAX_PARALLEL)
            .build()?;
        Ok(Self {
            podcasts: super::podcasts::Podcasts::new(client.clone()),
            client,
            mirrors,
            preferred: AtomicUsize::new(0),
            pages: Mutex::default(),
            permits: Semaphore::new(MAX_PARALLEL),
        })
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
        max: usize,
    ) -> Result<(usize, T), StatusCode> {
        let start = self.preferred.load(Ordering::Relaxed);
        for step in 0..self.mirrors.len() {
            let index = (start + step) % self.mirrors.len();
            let result = async {
                let mut response = self
                    .client
                    .get(format!("{}{path}", self.mirrors[index]))
                    .query(query)
                    .send()
                    .await
                    .ok()?;
                if !response.status().is_success()
                    || response.content_length().is_some_and(|n| n > max as u64)
                {
                    return None;
                }
                let mut buf = Vec::new();
                while let Some(chunk) = response.chunk().await.ok()? {
                    if chunk.len() > max - buf.len() {
                        return None;
                    }
                    buf.extend_from_slice(&chunk);
                }
                serde_json::from_slice::<T>(&buf).ok()
            }
            .await;
            if let Some(value) = result {
                self.preferred.store(index, Ordering::Relaxed);
                return Ok((index, value));
            }
        }
        Err(StatusCode::BAD_GATEWAY)
    }

    // Identical in-flight catalogue requests share the result, including failures.
    // No lock is held during network I/O. Expiration is bounded even on cancellation.
    fn cache_entry(&self, search: Search) -> CachedPage {
        let mut cache = self.pages.lock().expect("Radio Browser cache lock");
        cache.retain(|e| {
            let ttl = match e.page.get() {
                Some(Err(_)) => ERROR_TTL,
                _ => PAGE_TTL,
            };
            e.created.elapsed() < ttl
        });
        if let Some(entry) = cache.iter().find(|entry| entry.search == search) {
            return entry.page.clone();
        }
        if cache.len() == MAX_PAGES {
            cache.pop_front();
        }
        let page = Arc::new(OnceCell::new());
        cache.push_back(Entry {
            search,
            created: Instant::now(),
            page: page.clone(),
        });
        page
    }

    pub async fn stations(
        &self,
        name: &str,
        tag: &str,
        offset: u32,
        limit: u32,
    ) -> Result<Page, StatusCode> {
        if name.chars().count() > 160
            || tag.chars().count() > 120
            || !(1..=60).contains(&limit)
            || offset.checked_add(limit).is_none()
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        let name = name.trim();
        let tag = tag.trim();
        let entry = self.cache_entry(Search {
            name: name.to_owned(),
            tag: tag.to_owned(),
            offset,
            limit,
        });
        entry
            .get_or_init(|| async {
                let _permit = self
                    .permits
                    .try_acquire()
                    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
                let skip = offset.to_string();
                let count = (limit + 1).to_string();
                let mut query: Vec<(&str, &str)> = vec![
                    ("hidebroken", "true"),
                    ("order", "name"),
                    ("offset", &skip),
                    ("limit", &count),
                ];
                if !name.is_empty() {
                    query.push(("name", name));
                }
                if !tag.is_empty() {
                    query.push(("tag", tag));
                }
                let (_, rows): (_, Vec<serde_json::Value>) =
                    self.get("/json/stations/search", &query, MAX_JSON).await?;
                let more = rows.len() > limit as usize;
                // Radio Browser offsets count all upstream rows, including malformed rows we omit.
                let stations = rows
                    .into_iter()
                    .take(limit as usize)
                    .filter_map(|row| serde_json::from_value::<Listing>(row).ok()?.station())
                    .collect();
                Ok(Page {
                    stations,
                    next_offset: more.then_some(offset + limit),
                })
            })
            .await
            .clone()
    }

    pub async fn locate(&self, id: Uuid) -> Result<Stream, StatusCode> {
        self.resolve(id, false).await
    }

    pub async fn play(&self, id: Uuid) -> Result<Stream, StatusCode> {
        self.resolve(id, true).await
    }

    async fn resolve(&self, id: Uuid, count_click: bool) -> Result<Stream, StatusCode> {
        let _permit = self
            .permits
            .try_acquire()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let (index, rows): (_, Vec<Listing>) = self
            .get(
                &format!("/json/stations/byuuid/{id}"),
                &[("hidebroken", "true"), ("limit", "1")],
                64 * 1024,
            )
            .await?;
        let stream = rows
            .into_iter()
            .find(|row| Uuid::parse_str(&row.stationuuid).ok() == Some(id))
            .and_then(|row| row.stream())
            .ok_or(StatusCode::NOT_FOUND)?;
        // Count once at play time on the same mirror. A failed counter must not
        // interrupt a resolved stream or be retried as a second listening event.
        if count_click {
            let _ = self
                .client
                .get(format!("{}/json/url/{id}", self.mirrors[index]))
                .timeout(Duration::from_secs(1))
                .send()
                .await;
        }
        Ok(stream)
    }
}

#[cfg(test)]
#[path = "directory_tests.rs"]
mod tests;
