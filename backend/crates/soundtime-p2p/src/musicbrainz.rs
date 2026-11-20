//! Shared MusicBrainz recording resolution for imports and administrative enrichment.
use anyhow::{bail, ensure, Result};
use reqwest::{redirect, Client, Response, StatusCode, Url};
use serde::{Deserialize, Deserializer, Serialize};
use std::time::Duration;
use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;

mod ranking;
#[cfg(test)]
mod tests;

const MB_BASE_URL: &str = "https://musicbrainz.org/ws/2";
const MB_USER_AGENT: &str = "SoundTime/0.1.0 (https://github.com/CICCADA-CORP/SoundTime)";
const PAGE_SIZE: usize = 25;
static NEXT_REQUEST: Mutex<Option<Instant>> = Mutex::const_new(None);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    #[serde(rename = "sort-name")]
    pub sort_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtistCredit {
    pub name: Option<String>,
    #[serde(default)]
    pub joinphrase: String,
    pub artist: Artist,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Release {
    pub id: String,
    pub title: String,
    pub date: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    #[serde(default)]
    pub count: i32,
}

fn list<'de, D, T>(input: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(input)?.unwrap_or_default())
}

fn search_score<'de, D: Deserializer<'de>>(input: D) -> Result<u8, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Score {
        Number(u8),
        Text(String),
    }
    match Option::<Score>::deserialize(input)? {
        Some(Score::Number(n)) => Ok(n),
        Some(Score::Text(s)) => s.parse().map_err(serde::de::Error::custom),
        None => Ok(0),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recording {
    pub id: String,
    pub title: String,
    pub length: Option<u64>,
    pub disambiguation: Option<String>,
    #[serde(rename = "artist-credit", default, deserialize_with = "list")]
    pub artist_credit: Vec<ArtistCredit>,
    #[serde(default, deserialize_with = "list")]
    pub releases: Vec<Release>,
    #[serde(default, deserialize_with = "list")]
    pub tags: Vec<Tag>,
    #[serde(default, deserialize_with = "list")]
    pub isrcs: Vec<String>,
    #[serde(rename = "first-release-date")]
    pub first_release_date: Option<String>,
    #[serde(default, deserialize_with = "search_score")]
    pub score: u8,
}

impl Recording {
    pub fn credited_artist(&self) -> String {
        self.artist_credit
            .iter()
            .map(|c| {
                let name = c
                    .name
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&c.artist.name);
                format!("{}{}", name, c.joinphrase)
            })
            .collect()
    }

    fn prefer_album(&mut self, album: Option<&str>) {
        self.releases.sort_by(|a, b| {
            let known = |release: &Release| release.date.as_ref().is_some_and(|s| !s.is_empty());
            known(b).cmp(&known(a)).then_with(|| a.date.cmp(&b.date))
        });
        if let Some(album) = album.filter(|s| !s.trim().is_empty()) {
            let name = ranking::normalize(album);
            self.releases
                .sort_by_key(|release| ranking::normalize(&release.title) != name);
        }
    }
}

#[derive(Debug)]
pub enum RecordingMatch {
    Reliable(Recording),
    Ambiguous(Vec<Recording>),
    Absent,
}

#[derive(Deserialize)]
struct Page {
    recordings: Vec<Recording>,
    count: Option<usize>,
}

impl Page {
    fn truncated(&self) -> bool {
        self.count.map_or(self.recordings.len() >= PAGE_SIZE, |n| {
            n > self.recordings.len()
        })
    }
}

struct Hints<'a> {
    isrc: Option<&'a str>,
    title: &'a str,
    artist: Option<&'a str>,
    album: Option<&'a str>,
    duration_ms: Option<u64>,
}

impl Hints<'_> {
    fn title_only(&self) -> bool {
        self.artist.is_none() && self.album.is_none() && self.duration_ms.is_none()
    }
}

fn retry_delay(response: &Response, attempt: u32) -> Duration {
    let wait = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|h| h.to_str().ok());
    if let Some(value) = wait {
        if let Ok(seconds) = value.parse::<u64>() {
            return Duration::from_secs(seconds);
        }
        if let Ok(date) = chrono::DateTime::parse_from_rfc2822(value) {
            return (date.to_utc() - chrono::Utc::now())
                .to_std()
                .unwrap_or_default();
        }
    }
    Duration::from_secs(1 << attempt)
}

/// HTTP client confined to the configured MusicBrainz API on every redirect.
pub struct MusicBrainzHttp(Client);

impl MusicBrainzHttp {
    pub fn new(user_agent: &str) -> Result<Self, reqwest::Error> {
        Client::builder()
            .user_agent(user_agent)
            .timeout(Duration::from_secs(12))
            .redirect(redirect::Policy::custom(|attempt| {
                if attempt.previous().len() <= 5
                    && attempt
                        .previous()
                        .first()
                        .is_some_and(|initial| allows_redirect(initial, attempt.url()))
                {
                    attempt.follow()
                } else {
                    attempt.error("unexpected MusicBrainz redirect")
                }
            }))
            .build()
            .map(Self)
    }
}

fn allows_redirect(initial: &Url, target: &Url) -> bool {
    if initial.origin() != target.origin()
        || !target.username().is_empty()
        || target.password().is_some()
    {
        return false;
    }
    let from = initial.path().trim_end_matches('/');
    let to = target.path().trim_end_matches('/');
    // MusicBrainz redirects merged IDs to the canonical entity in the same collection.
    from == to
        || matches!(
            (from.rsplit_once('/'), to.rsplit_once('/')),
            (Some((collection, old)), Some((destination, new)))
                if collection == destination && Uuid::parse_str(old).is_ok() && Uuid::parse_str(new).is_ok()
        )
}

fn endpoint(base: &str, collection: &str, id: Option<Uuid>) -> Result<Url> {
    let mut url = Url::parse(base)?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.has_host()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "invalid MusicBrainz API base"
    );
    let mut path = url
        .path_segments_mut()
        .map_err(|_| anyhow::anyhow!("invalid MusicBrainz API path"))?;
    path.pop_if_empty().push(collection);
    if let Some(id) = id {
        path.push(id.hyphenated().encode_lower(&mut Uuid::encode_buffer()));
    }
    drop(path);
    Ok(url)
}

async fn request(client: &MusicBrainzHttp, url: Url, query: &[(&str, &str)]) -> Result<Response> {
    for attempt in 0..3 {
        // Keep response headers inside the critical section so no other caller
        // can race past a new provider-wide Retry-After embargo.
        let mut next = NEXT_REQUEST.lock().await;
        if let Some(when) = *next {
            ensure!(
                when.saturating_duration_since(Instant::now()) <= Duration::from_secs(15),
                "MusicBrainz requested a longer cooldown; retry later"
            );
            tokio::time::sleep_until(when).await;
        }
        *next = Some(Instant::now() + Duration::from_secs(1));
        let response = client
            .0
            .get(url.clone())
            .query(query)
            .timeout(Duration::from_secs(12))
            .send()
            .await?;
        if !matches!(
            response.status(),
            StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
        ) {
            return Ok(response);
        }
        let delay = retry_delay(&response, attempt).max(Duration::from_secs(1));
        let now = Instant::now();
        *next = Some(
            now.checked_add(delay)
                .unwrap_or(now + Duration::from_secs(365 * 86400)),
        );
        if attempt == 2 || delay > Duration::from_secs(15) {
            bail!(
                "MusicBrainz returned {} (retry after {} seconds)",
                response.status(),
                delay.as_secs()
            );
        }
    }
    unreachable!()
}

pub async fn get_artist(client: &MusicBrainzHttp, base_url: &str, mbid: &str) -> Result<Response> {
    let url = endpoint(base_url, "artist", Some(Uuid::parse_str(mbid)?))?;
    Ok(
        request(client, url, &[("inc", "url-rels"), ("fmt", "json")])
            .await?
            .error_for_status()?,
    )
}

pub async fn get_recording(
    client: &MusicBrainzHttp,
    base_url: &str,
    id: Uuid,
) -> Result<Option<Recording>> {
    let url = endpoint(base_url, "recording", Some(id))?;
    let response = request(
        client,
        url,
        &[
            ("inc", "artist-credits+releases+tags+isrcs"),
            ("fmt", "json"),
        ],
    )
    .await?;
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response.error_for_status()?;
    let resolved_id = response
        .url()
        .path_segments()
        .and_then(|mut parts| parts.next_back())
        .and_then(|s| Uuid::parse_str(s).ok());
    let mut recording: Recording = response.json().await?;
    ensure!(
        Some(Uuid::parse_str(&recording.id)?) == resolved_id && !recording.title.trim().is_empty(),
        "MusicBrainz returned an inconsistent recording identity"
    );
    if recording
        .first_release_date
        .as_deref()
        .unwrap_or("")
        .is_empty()
    {
        recording.first_release_date = recording
            .releases
            .iter()
            .filter_map(|r| r.date.as_ref())
            .filter(|s| !s.is_empty())
            .min()
            .cloned();
    }
    recording.prefer_album(None);
    Ok(Some(recording))
}

fn quoted(field: &str, value: &str) -> String {
    // Lucene phrase literals escape both backslash and the closing quote.
    let bounded: String = value.trim().chars().take(256).collect();
    format!(
        r#"{field}:"{}""#,
        bounded.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

fn text_queries(hints: &Hints<'_>) -> Vec<String> {
    let full = quoted("recording", hints.title);
    let core = quoted("recording", ranking::base_title(hints.title));
    let mut queries = Vec::new();
    if let Some(artist) = hints.artist {
        let artist = quoted("artist", artist);
        queries.push(format!("{full} AND {artist}"));
        if core != full {
            queries.push(format!("{core} AND {artist}"));
        }
    } else if core != full {
        queries.push(full);
    }
    if let Some(album) = hints.album {
        queries.push(format!("{core} AND {}", quoted("release", album)));
    }
    if let (None, Some(ms)) = (hints.artist, hints.duration_ms) {
        queries.push(format!(
            "{core} AND dur:[{} TO {}]",
            ms.saturating_sub(7000),
            ms.saturating_add(7000)
        ));
    }
    queries.push(core);
    queries
}

async fn search(client: &MusicBrainzHttp, base: &str, query: &str) -> Result<Page> {
    search_from(client, base, query, 0).await
}

async fn search_from(
    client: &MusicBrainzHttp,
    base: &str,
    query: &str,
    offset: usize,
) -> Result<Page> {
    let url = endpoint(base, "recording", None)?;
    let limit = PAGE_SIZE.to_string();
    let offset_text = offset.to_string();
    let mut params = vec![("query", query), ("fmt", "json"), ("limit", limit.as_str())];
    if offset > 0 {
        params.push(("offset", &offset_text));
    }
    Ok(request(client, url, &params)
        .await?
        .error_for_status()?
        .json()
        .await?)
}

async fn details(
    client: &MusicBrainzHttp,
    base: &str,
    found: Recording,
    album: Option<&str>,
    isrc: Option<&str>,
) -> Result<Option<Recording>> {
    let Some(mut full) = get_recording(client, base, Uuid::parse_str(&found.id)?).await? else {
        return Ok(None);
    };
    if isrc.is_some_and(|code| {
        !full
            .isrcs
            .iter()
            .any(|s| isrc_code(s).as_deref() == Some(code))
    }) {
        return Ok(None);
    }
    full.prefer_album(album);
    full.score = found.score;
    if let Some(date) = found.first_release_date.filter(|s| !s.is_empty()) {
        let first = full.first_release_date.get_or_insert_with(|| date.clone());
        if date < *first {
            *first = date;
        }
    }
    Ok(Some(full))
}

fn isrc_code(raw: &str) -> Option<String> {
    let code: String = raw
        .trim()
        .to_ascii_uppercase()
        .chars()
        .filter(|c| *c != '-')
        .collect();
    let s = code.as_bytes();
    (s.len() == 12
        && s[..2].iter().all(u8::is_ascii_alphabetic)
        && s[2..5].iter().all(u8::is_ascii_alphanumeric)
        && s[5..].iter().all(u8::is_ascii_digit))
    .then_some(code)
}

pub async fn resolve_recording(
    client: &MusicBrainzHttp,
    base_url: &str,
    title: &str,
    artist: Option<&str>,
    album: Option<&str>,
    duration_ms: Option<u64>,
) -> Result<RecordingMatch> {
    resolve_tagged_recording(
        client,
        base_url,
        None,
        None,
        title,
        artist,
        album,
        duration_ms,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn resolve_tagged_recording(
    client: &MusicBrainzHttp,
    base_url: &str,
    recording_id: Option<Uuid>,
    isrc: Option<&str>,
    title: &str,
    artist: Option<&str>,
    album: Option<&str>,
    duration_ms: Option<u64>,
) -> Result<RecordingMatch> {
    let code = isrc.and_then(isrc_code);
    let hints = Hints {
        isrc: code.as_deref(),
        title,
        artist: artist.filter(|s| !s.trim().is_empty()),
        album: album.filter(|s| !s.trim().is_empty()),
        duration_ms: duration_ms.filter(|n| *n > 0),
    };
    if let Some(id) = recording_id {
        if let Some(mut recording) = get_recording(client, base_url, id).await? {
            recording.prefer_album(hints.album);
            return Ok(RecordingMatch::Reliable(recording));
        }
    }
    if let Some(ref code) = code {
        let page = search(client, base_url, &quoted("isrc", code)).await?;
        let truncated = page.truncated();
        let mut matches = Vec::new();
        for r in page.recordings {
            if !r.isrcs.is_empty() && !r.isrcs.iter().any(|s| isrc_code(s).as_ref() == Some(code)) {
                continue;
            }
            ensure!(
                Uuid::parse_str(&r.id).is_ok(),
                "Invalid recording MBID in ISRC response"
            );
            if !matches.iter().any(|v: &Recording| v.id == r.id) {
                matches.push(r);
            }
        }
        if !matches.is_empty() {
            let unique = matches.len() == 1 && !truncated;
            let found = if unique {
                matches.remove(0)
            } else {
                match ranking::choose(&hints, &matches) {
                    RecordingMatch::Reliable(r) if !truncated => r,
                    RecordingMatch::Ambiguous(r) => return Ok(RecordingMatch::Ambiguous(r)),
                    _ => {
                        matches.truncate(5);
                        return Ok(RecordingMatch::Ambiguous(matches));
                    }
                }
            };
            if let Some(full) = details(client, base_url, found, hints.album, Some(code)).await? {
                return Ok(if unique {
                    RecordingMatch::Reliable(full)
                } else {
                    ranking::choose(&hints, &[full])
                });
            }
        }
    }
    if ranking::normalize(title).is_empty() {
        return Ok(RecordingMatch::Absent);
    }
    let mut seen: Vec<Recording> = Vec::new();
    let mut complete_ids = std::collections::HashSet::new();
    let mut decision = RecordingMatch::Absent;
    let mut next_offset = None;
    for query in text_queries(&hints) {
        let page = search(client, base_url, &query).await?;
        let truncated = page.truncated();
        next_offset = truncated.then_some(page.recordings.len().min(PAGE_SIZE));
        merge_recordings(page, &mut seen, &mut complete_ids)?;
        decision = ranking::choose(&hints, &seen);
        if let RecordingMatch::Reliable(found) = decision {
            if !complete_ids.contains(&found.id) {
                decision = RecordingMatch::Ambiguous(vec![found]);
            } else if let Some(full) = details(client, base_url, found, hints.album, None).await? {
                // A stale search hit can refer to a recording whose canonical title,
                // credits or version have since changed.
                return Ok(ranking::choose(&hints, &[full]));
            } else {
                decision = RecordingMatch::Absent;
            }
        }
    }
    if hints.title_only() && matches!(decision, RecordingMatch::Absent) {
        if let Some(offset) = next_offset {
            // A full page of partial title matches is inconclusive. Spend at most
            // one additional request on it, and never auto-tag a title-only hit.
            let core = ranking::base_title(hints.title);
            let word = ranking::normalize(core);
            let mut query = quoted("recording", core);
            let repeats = !word.contains(' ')
                && word.len() <= 100
                && seen.iter().any(|r| {
                    let title = ranking::normalize(&r.title);
                    let mut words = title.split_whitespace();
                    words.next() == Some(&word) && words.next() == Some(&word)
                });
            let offset = if repeats {
                let phrase = quoted("recording", &format!("{word} {word}"));
                query = format!("{query} AND NOT {phrase}");
                0
            } else {
                offset
            };
            if repeats || offset > 0 {
                let page = search_from(client, base_url, &query, offset).await?;
                merge_recordings(page, &mut seen, &mut complete_ids)?;
            }
            return Ok(match ranking::choose(&hints, &seen) {
                RecordingMatch::Ambiguous(rows) => RecordingMatch::Ambiguous(rows),
                _ => RecordingMatch::Ambiguous(Vec::new()),
            });
        }
    }
    Ok(decision)
}

fn merge_recordings(
    page: Page,
    seen: &mut Vec<Recording>,
    complete_ids: &mut std::collections::HashSet<String>,
) -> Result<()> {
    let truncated = page.truncated();
    for recording in page.recordings.into_iter().take(PAGE_SIZE) {
        ensure!(
            Uuid::parse_str(&recording.id).is_ok(),
            "Invalid recording MBID in search response"
        );
        if !truncated {
            complete_ids.insert(recording.id.clone());
        }
        if let Some(previous) = seen.iter_mut().find(|r| r.id == recording.id) {
            previous.score = previous.score.max(recording.score);
        } else {
            seen.push(recording);
        }
    }
    Ok(())
}

// Preserve the existing compact return type for P2P callers.
#[derive(Debug, Clone)]
pub struct MusicBrainzRecording {
    pub id: String,
    pub title: String,
    pub artist_name: Option<String>,
    pub release_title: Option<String>,
    pub year: Option<i16>,
    pub score: u8,
}

pub struct MusicBrainzClient {
    http: MusicBrainzHttp,
    base_url: String,
}

impl MusicBrainzClient {
    pub fn new() -> Self {
        Self {
            http: MusicBrainzHttp::new(MB_USER_AGENT)
                .expect("failed to build MusicBrainz HTTP client"),
            base_url: std::env::var("MUSICBRAINZ_BASE_URL")
                .unwrap_or_else(|_| MB_BASE_URL.to_owned()),
        }
    }

    pub async fn lookup_recording(
        &self,
        title: &str,
        artist: &str,
    ) -> Option<MusicBrainzRecording> {
        match resolve_recording(&self.http, &self.base_url, title, Some(artist), None, None).await {
            Ok(RecordingMatch::Reliable(r)) => {
                let credit = r.credited_artist();
                let rls = r.releases.first();
                let date = r
                    .first_release_date
                    .as_deref()
                    .or_else(|| rls.and_then(|r| r.date.as_deref()));
                Some(MusicBrainzRecording {
                    artist_name: (!credit.is_empty()).then_some(credit),
                    release_title: rls.map(|r| r.title.clone()),
                    year: date.and_then(|s| s.split('-').next()?.parse().ok()),
                    score: r.score,
                    id: r.id,
                    title: r.title,
                })
            }
            Err(e) => {
                tracing::warn!(error=%e, "MusicBrainz lookup unavailable");
                None
            }
            Ok(_) => None,
        }
    }
}

impl Default for MusicBrainzClient {
    fn default() -> Self {
        Self::new()
    }
}
