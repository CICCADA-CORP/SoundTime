//! Apple supplies the show directory; publishers supply RSS metadata and host the audio.
mod rss;
use super::{directory::web_url, relay::open_public, require_enabled, AuthUser, Directory};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use soundtime_db::AppState;
use std::{
    collections::{HashSet, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{OnceCell, Semaphore};
use url::Url;

const APPLE: &str = "https://itunes.apple.com";
const CHARTS: &str = "https://rss.marketingtools.apple.com";
const SHOW_PAGE_SIZE: usize = 25;
const MAX_SEARCH_SHOWS: usize = 200;
const MAX_JSON: usize = 1024 * 1024;
const MAX_XML: usize = 2 * 1024 * 1024;
const BAD: StatusCode = StatusCode::BAD_GATEWAY;

#[derive(Serialize, Clone)]
pub(super) struct Shows {
    shows: Vec<Show>,
    next_offset: Option<u32>,
}
#[derive(Serialize, Clone)]
struct Show {
    id: String,
    title: String,
    author: String,
    artwork_url: Option<String>,
    genre: String,
}
#[derive(Serialize, Clone)]
pub(super) struct Episodes {
    show: Show,
    episodes: Vec<rss::Episode>,
    next_offset: Option<u32>,
    truncated: bool,
}
struct Archive {
    show: Show,
    episodes: Vec<rss::Episode>,
    truncated: bool,
}
impl Archive {
    fn page(&self, offset: u32) -> Episodes {
        let start = (offset as usize).min(self.episodes.len());
        let end = (start + 50).min(self.episodes.len());
        Episodes {
            show: self.show.clone(),
            episodes: self.episodes[start..end].to_vec(),
            next_offset: (end < self.episodes.len()).then_some(end as u32),
            truncated: self.truncated,
        }
    }
}

#[derive(Deserialize)]
pub(super) struct Browse {
    #[serde(default)]
    q: String,
    country: Option<String>,
    #[serde(default)]
    offset: u32,
}
impl Browse {
    fn market(&self) -> Result<String, StatusCode> {
        let code = self.country.as_deref().unwrap_or("US");
        if code.len() != 2 || !code.bytes().all(|c| c.is_ascii_alphabetic()) {
            return Err(StatusCode::BAD_REQUEST);
        }
        Ok(code.to_ascii_uppercase())
    }
}

type Cell<T> = Arc<OnceCell<Result<T, StatusCode>>>;
struct Cache<T, const N: usize>(Mutex<VecDeque<(String, Instant, Cell<T>)>>);
impl<T, const N: usize> Cache<T, N> {
    fn new() -> Self {
        Self(Mutex::new(VecDeque::new()))
    }
    fn get(&self, key: String) -> Result<Cell<T>, StatusCode> {
        let mut cache = self.0.lock().unwrap();
        cache.retain(|(_, created, value)| {
            created.elapsed()
                < Duration::from_secs(if matches!(value.get(), Some(Err(_))) {
                    5
                } else {
                    300
                })
        });
        if let Some((_, _, value)) = cache.iter().find(|(k, _, _)| k == &key) {
            return Ok(value.clone());
        }
        if cache.len() == N {
            let i = cache
                .iter()
                .position(|(_, _, v)| v.get().is_some() || Arc::strong_count(v) == 1)
                .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
            cache.remove(i);
        }
        let cell = Arc::new(OnceCell::new());
        cache.push_back((key, Instant::now(), Arc::clone(&cell)));
        Ok(cell)
    }
}

pub(super) struct Podcasts {
    client: Client,
    apple: String,
    charts: String,
    shows: Cache<Vec<Show>, 32>,
    episodes: Cache<Arc<Archive>, 12>,
    slots: Semaphore,
    apple_calls: Mutex<VecDeque<Instant>>,
}
impl Podcasts {
    pub(super) fn new(client: Client) -> Self {
        Self {
            client,
            apple: APPLE.into(),
            charts: CHARTS.into(),
            shows: Cache::new(),
            episodes: Cache::new(),
            slots: Semaphore::new(6),
            apple_calls: Mutex::default(),
        }
    }

    async fn apple_json(&self, url: Url) -> Result<Value, StatusCode> {
        {
            let mut calls = self.apple_calls.lock().unwrap();
            calls.retain(|time| time.elapsed() < Duration::from_secs(60));
            if calls.len() >= 20 {
                return Err(StatusCode::TOO_MANY_REQUESTS);
            }
            calls.push_back(Instant::now());
        }
        let response = self.client.get(url).send().await.map_err(|_| BAD)?;
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        if !response.status().is_success() {
            return Err(BAD);
        }
        let (bytes, truncated) = read_prefix(response, MAX_JSON).await?;
        if truncated {
            return Err(BAD);
        }
        serde_json::from_slice(&bytes).map_err(|_| BAD)
    }

    async fn browse(&self, country: &str, term: &str, offset: u32) -> Result<Shows, StatusCode> {
        if term.len() > 320 || term.chars().any(char::is_control) {
            return Err(StatusCode::BAD_REQUEST);
        }
        let entry = self.shows.get(format!("{country}:{term}"))?;
        let all = entry
            .get_or_init(|| async {
                let _slot = self
                    .slots
                    .try_acquire()
                    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
                let chart = term.is_empty();
                let url = if chart {
                    Url::parse(&format!(
                        "{}/api/v2/{}/podcasts/top/{SHOW_PAGE_SIZE}/podcasts.json",
                        self.charts,
                        country.to_ascii_lowercase()
                    ))
                    .map_err(|_| BAD)?
                } else {
                    let mut url = Url::parse(&format!("{}/search", self.apple)).map_err(|_| BAD)?;
                    url.query_pairs_mut().extend_pairs([
                        ("country", country),
                        ("media", "podcast"),
                        ("entity", "podcast"),
                        ("term", term),
                        ("limit", &MAX_SEARCH_SHOWS.to_string()),
                    ]);
                    url
                };
                let data = match self.apple_json(url).await {
                    Err(BAD) if chart => {
                        // Apple's legacy chart is served independently by the iTunes storefront.
                        let backup = Url::parse(&format!(
                            "{}/{}/rss/toppodcasts/limit={SHOW_PAGE_SIZE}/json",
                            self.apple,
                            country.to_ascii_lowercase()
                        ))
                        .map_err(|_| BAD)?;
                        let fallback = self.apple_json(backup).await?;
                        let feed = fallback.get("feed").filter(|f| f.is_object()).ok_or(BAD)?;
                        let Some(entries) = feed.get("entry") else {
                            return Ok(Vec::new());
                        };
                        let mut seen = HashSet::new();
                        let rows: Vec<_> = match entries.as_array() {
                            Some(a) => a.iter().collect(),
                            None if entries.is_object() => vec![entries],
                            None => return Err(BAD),
                        };
                        return Ok(rows
                            .into_iter()
                            .filter_map(legacy_show)
                            .filter(|s| seen.insert(s.id.clone()))
                            .take(SHOW_PAGE_SIZE)
                            .collect());
                    }
                    other => other?,
                };
                let rows = if chart {
                    data.pointer("/feed/results")
                } else {
                    data.get("results")
                }
                .and_then(Value::as_array)
                .ok_or(BAD)?;
                let mut seen = HashSet::new();
                Ok(rows
                    .iter()
                    .filter_map(|row| show(row, chart))
                    .filter(|s| seen.insert(s.id.clone()))
                    .take(if chart {
                        SHOW_PAGE_SIZE
                    } else {
                        MAX_SEARCH_SHOWS
                    })
                    .collect())
            })
            .await
            .as_ref()
            .map_err(|e| *e)?;
        let start = (offset as usize).min(all.len());
        let end = (start + SHOW_PAGE_SIZE).min(all.len());
        Ok(Shows {
            shows: all[start..end].to_vec(),
            next_offset: (end < all.len()).then_some(end as u32),
        })
    }

    async fn feed(&self, country: &str, id: &str) -> Result<Arc<Archive>, StatusCode> {
        if !valid_id(id) {
            return Err(StatusCode::BAD_REQUEST);
        }
        self.episodes
            .get(format!("{country}:{id}"))?
            .get_or_init(|| async {
                let _slot = self
                    .slots
                    .try_acquire()
                    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
                let mut url = Url::parse(&format!("{}/lookup", self.apple)).map_err(|_| BAD)?;
                url.query_pairs_mut().extend_pairs([
                    ("id", id),
                    ("entity", "podcast"),
                    ("country", country),
                ]);
                let data = self.apple_json(url).await?;
                let row = data["results"]
                    .as_array()
                    .ok_or(BAD)?
                    .iter()
                    .find(|r| {
                        r["kind"] == "podcast"
                            && r["collectionId"]
                                .as_u64()
                                .is_some_and(|n| n.to_string() == id)
                    })
                    .ok_or(StatusCode::NOT_FOUND)?;
                let show = show(row, false).ok_or(BAD)?;
                // Apple also lists shows that publish no public RSS feed.
                let Some(feed) = row["feedUrl"].as_str().filter(|url| !url.trim().is_empty())
                else {
                    return Ok(Arc::new(Archive {
                        show,
                        episodes: Vec::new(),
                        truncated: false,
                    }));
                };
                let feed = web_url(feed).ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
                tokio::time::timeout(Duration::from_secs(12), async {
                    let response = open_public(Url::parse(&feed).map_err(|_| BAD)?).await?;
                    read_archive(response, show).await
                })
                .await
                .map_err(|_| StatusCode::GATEWAY_TIMEOUT)?
            })
            .await
            .clone()
    }
}

async fn read_archive(response: Response, show: Show) -> Result<Arc<Archive>, StatusCode> {
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if mime.starts_with("audio/")
        || mime.starts_with("video/")
        || mime.starts_with("image/")
        || mime.starts_with("text/html")
    {
        return Err(BAD);
    }
    let (body, truncated) = read_prefix(response, MAX_XML).await?;
    // Parsing a large feed is CPU work; the caller holds one of the six catalogue slots.
    let (episodes, truncated) =
        tokio::task::spawn_blocking(move || rss::episodes(&body, truncated))
            .await
            .map_err(|_| BAD)??;
    Ok(Arc::new(Archive {
        show,
        episodes,
        truncated,
    }))
}

fn valid_id(raw: &str) -> bool {
    !raw.starts_with('0')
        && raw.len() <= 20
        && raw.bytes().all(|c| c.is_ascii_digit())
        && raw.parse::<u64>().is_ok()
}

fn field(row: &Value, key: &str) -> String {
    row[key]
        .as_str()
        .unwrap_or_default()
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect()
}

fn show(row: &Value, chart: bool) -> Option<Show> {
    let (id, title, genre) = if chart {
        if row["kind"] != "podcasts" {
            return None;
        }
        let genres = row["genres"]
            .as_array()?
            .iter()
            .map(|g| field(g, "name"))
            .find(|g| !g.is_empty() && g != "Podcasts")
            .unwrap_or_default();
        (row["id"].as_str()?.to_string(), field(row, "name"), genres)
    } else {
        if row["kind"] != "podcast" {
            return None;
        }
        (
            row["collectionId"].as_u64()?.to_string(),
            field(row, "collectionName"),
            field(row, "primaryGenreName"),
        )
    };
    if !valid_id(&id) || title.is_empty() {
        return None;
    }
    let artwork_url = ["artworkUrl600", "artworkUrl100"].iter().find_map(|k| {
        row[*k]
            .as_str()
            .and_then(web_url)
            .filter(|u| u.starts_with("https://"))
    });
    Some(Show {
        id,
        title,
        author: field(row, "artistName"),
        artwork_url,
        genre,
    })
}

fn legacy_show(row: &Value) -> Option<Show> {
    let id = row.pointer("/id/attributes/im:id")?.as_str()?;
    let title = field(&row["im:name"], "label");
    if !valid_id(id) || title.is_empty() {
        return None;
    }
    let artwork_url = row["im:image"]
        .as_array()
        .into_iter()
        .flatten()
        .rev()
        .find_map(|i| {
            i["label"]
                .as_str()
                .and_then(web_url)
                .filter(|u| u.starts_with("https://"))
        });
    Some(Show {
        id: id.to_owned(),
        title,
        author: field(&row["im:artist"], "label"),
        genre: field(&row["category"]["attributes"], "label"),
        artwork_url,
    })
}

async fn read_prefix(mut response: Response, max: usize) -> Result<(Vec<u8>, bool), StatusCode> {
    let mut bytes = Vec::new();
    let known_long = response.content_length().is_some_and(|n| n > max as u64);
    while let Some(chunk) = response.chunk().await.map_err(|_| BAD)? {
        let left = max - bytes.len();
        bytes.extend_from_slice(&chunk[..chunk.len().min(left)]);
        if chunk.len() > left || (known_long && bytes.len() == max) {
            return Ok((bytes, true));
        }
    }
    Ok((bytes, false))
}

pub(super) async fn shows(
    State(state): State<Arc<AppState>>,
    user: Option<Extension<AuthUser>>,
    Extension(directory): Extension<Arc<Directory>>,
    Query(query): Query<Browse>,
) -> Result<Json<Shows>, StatusCode> {
    require_enabled(&state.db, user).await?;
    directory
        .podcasts
        .browse(&query.market()?, query.q.trim(), query.offset)
        .await
        .map(Json)
}
pub(super) async fn episodes(
    State(state): State<Arc<AppState>>,
    user: Option<Extension<AuthUser>>,
    Extension(directory): Extension<Arc<Directory>>,
    Path(id): Path<String>,
    Query(query): Query<Browse>,
) -> Result<Json<Episodes>, StatusCode> {
    require_enabled(&state.db, user).await?;
    directory
        .podcasts
        .feed(&query.market()?, &id)
        .await
        .map(|archive| Json(archive.page(query.offset)))
}

#[cfg(test)]
mod tests;
