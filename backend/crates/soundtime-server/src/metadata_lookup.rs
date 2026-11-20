//! Metadata auto-fetch via MusicBrainz + Cover Art Archive.
//!
//! Queries MusicBrainz recording search to enrich track/album/artist metadata,
//! and Cover Art Archive to fetch cover images.
//!
//! ## Background task API
//!
//! The [`enrich_all_tracks_background`] function runs batch enrichment as an
//! async task, reporting progress through a [`MetadataTaskTrackerHandle`].
//! Admin handlers poll and dismiss the tracker via the `/admin/metadata/`
//! endpoints (see `api::admin`).
//!
//! ## Rate limiting
//!
//! Recording searches, ID lookups and artist lookups use the shared MusicBrainz request scheduler.

pub(crate) mod hints;
use reqwest::Client;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use soundtime_audio::StorageBackend;
use soundtime_db::entities::{album, artist, instance_setting, track};
use soundtime_p2p::musicbrainz::{self, MusicBrainzHttp, RecordingMatch};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[cfg(test)]
mod batch_tests;

/// MusicBrainz user-agent (required by their API policy) — defaults, overridden by instance settings
const DEFAULT_MB_USER_AGENT: &str = "SoundTime/0.1.0 (https://github.com/CICCADA-CORP/SoundTime)";

/// Build the base URL for the given domain, respecting SOUNDTIME_SCHEME env var.
fn instance_base_url(domain: &str) -> String {
    let scheme = std::env::var("SOUNDTIME_SCHEME").unwrap_or_else(|_| "https".to_string());
    format!("{scheme}://{domain}")
}
const DEFAULT_MB_BASE_URL: &str = "https://musicbrainz.org/ws/2";
const DEFAULT_CAA_BASE_URL: &str = "https://coverartarchive.org";

/// Rate limiter: MusicBrainz allows max 1 request per second.
/// We use 1100ms to stay safely under the limit.
const MB_RATE_LIMIT_MS: u64 = 1100;

/// Load MusicBrainz configuration from instance_settings or fall back to defaults.
async fn load_mb_config(db: &DatabaseConnection) -> (String, String, String) {
    let keys = [
        "musicbrainz_base_url",
        "musicbrainz_user_agent",
        "coverart_base_url",
    ];
    let rows = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.is_in(keys))
        .all(db)
        .await
        .unwrap_or_default();
    let value = |key: &str, default: &str| {
        rows.iter()
            .find(|r| r.key == key && !r.value.trim().is_empty())
            .map_or(default.to_owned(), |r| r.value.trim().to_owned())
    };
    (
        value(keys[0], DEFAULT_MB_BASE_URL),
        value(keys[1], DEFAULT_MB_USER_AGENT),
        value(keys[2], DEFAULT_CAA_BASE_URL),
    )
}

// ─── MusicBrainz API response types ────────────────────────────────

#[derive(Debug, Deserialize)]
struct CaaResponse {
    images: Vec<CaaImage>,
}

#[derive(Debug, Deserialize)]
struct CaaImage {
    front: bool,
    thumbnails: Option<CaaThumbnails>,
    image: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CaaThumbnails {
    #[serde(rename = "500")]
    size_500: Option<String>,
    #[serde(rename = "250")]
    size_250: Option<String>,
    large: Option<String>,
    small: Option<String>,
}

// ─── Public result type ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct MetadataCandidate {
    pub recording_mbid: String,
    pub title: String,
    pub artist_name: Option<String>,
    pub album_title: Option<String>,
    pub duration_ms: Option<u64>,
    pub disambiguation: Option<String>,
}

fn join_credit(recording: &musicbrainz::Recording) -> Option<String> {
    let mut credit = String::new();
    for artist in &recording.artist_credit {
        let name = artist
            .name
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&artist.artist.name);
        credit.push_str(name);
        credit.push_str(&artist.joinphrase);
    }
    let result = credit.trim();
    (!result.is_empty()).then(|| result.to_owned())
}

fn same_label(a: &str, b: &str) -> bool {
    fn key(s: &str) -> String {
        s.chars()
            .flat_map(char::to_lowercase)
            .filter(|c| c.is_alphanumeric())
            .collect()
    }
    !a.trim().is_empty() && key(a) == key(b)
}

impl From<musicbrainz::Recording> for MetadataCandidate {
    fn from(r: musicbrainz::Recording) -> Self {
        let artist_name = join_credit(&r);
        Self {
            recording_mbid: r.id,
            title: r.title,
            artist_name,
            album_title: r.releases.first().map(|v| v.title.clone()),
            duration_ms: r.length,
            disambiguation: r.disambiguation,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MetadataResult {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<MetadataCandidate>,
    pub track_id: Uuid,
    pub status: MetadataStatus,
    pub recording_mbid: Option<String>,
    pub corrected_title: Option<String>,
    pub artist_mbid: Option<String>,
    pub artist_name: Option<String>,
    pub album_mbid: Option<String>,
    pub album_title: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i16>,
    pub cover_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataStatus {
    Enriched,
    EnrichedByAi,
    NotFound,
    Ambiguous,
    AlreadyEnriched,
    Error,
}

// ─── Core lookup logic ──────────────────────────────────────────────

fn build_client() -> Result<Client, reqwest::Error> {
    build_client_with_ua(DEFAULT_MB_USER_AGENT)
}

fn build_client_with_ua(user_agent: &str) -> Result<Client, reqwest::Error> {
    Client::builder()
        .user_agent(user_agent)
        .timeout(std::time::Duration::from_secs(15))
        .build()
}

/// Fetch cover art from Cover Art Archive for a release MBID.
async fn fetch_cover_art_url(
    client: &Client,
    release_mbid: &str,
    caa_base_url: &str,
) -> Option<String> {
    // Rate limit: respect external API policies
    tokio::time::sleep(std::time::Duration::from_millis(MB_RATE_LIMIT_MS)).await;

    let resp = client
        .get(format!("{caa_base_url}/release/{release_mbid}"))
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let caa: CaaResponse = resp.json().await.ok()?;
    let front = caa.images.into_iter().find(|img| img.front)?;

    // Prefer 500px thumbnail, then large, then original
    front
        .thumbnails
        .as_ref()
        .and_then(|t| {
            t.size_500
                .clone()
                .or_else(|| t.large.clone())
                .or_else(|| t.size_250.clone())
        })
        .or(front.image)
}

/// Download cover image bytes from URL.
async fn download_cover(client: &Client, url: &str) -> Option<Vec<u8>> {
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.bytes().await.ok().map(|b| b.to_vec())
}

// ─── Artist bio/image enrichment via MusicBrainz + Wikipedia ────────

#[derive(Debug, Deserialize)]
struct MbArtistLookup {
    relations: Option<Vec<MbRelation>>,
}

#[derive(Debug, Deserialize)]
struct MbRelation {
    #[serde(rename = "type")]
    relation_type: Option<String>,
    url: Option<MbRelationUrl>,
}

#[derive(Debug, Deserialize)]
struct MbRelationUrl {
    resource: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WikipediaSummary {
    extract: Option<String>,
    thumbnail: Option<WikipediaThumbnail>,
}

#[derive(Debug, Deserialize)]
struct WikipediaThumbnail {
    source: Option<String>,
}

/// Wikidata sitelinks response for resolving QID → Wikipedia title.
#[derive(Debug, Deserialize)]
struct WikidataResponse {
    entities: Option<std::collections::HashMap<String, WikidataEntity>>,
}

#[derive(Debug, Deserialize)]
struct WikidataEntity {
    sitelinks: Option<std::collections::HashMap<String, WikidataSitelink>>,
}

#[derive(Debug, Deserialize)]
struct WikidataSitelink {
    title: Option<String>,
}

/// Fetch artist bio and image from MusicBrainz relations → Wikipedia.
/// Falls back to searching Wikipedia by artist name if no direct link.
/// Returns (bio, image_url).
async fn fetch_artist_bio_image(
    client: &Client,
    mb_http: &MusicBrainzHttp,
    artist_mbid: &str,
    artist_name: &str,
    mb_base_url: &str,
) -> (Option<String>, Option<String>) {
    let resp = match musicbrainz::get_artist(mb_http, mb_base_url, artist_mbid).await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            tracing::warn!("MB artist lookup returned {}", r.status());
            return (None, None);
        }
        Err(e) => {
            tracing::warn!("MB artist lookup failed: {e}");
            return (None, None);
        }
    };

    let lookup: MbArtistLookup = match resp.json().await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("MB artist lookup parse failed: {e}");
            return (None, None);
        }
    };

    let relations = lookup.relations.unwrap_or_default();

    // 2. Extract useful relations
    let mut wikipedia_url: Option<String> = None;
    let mut wikidata_url: Option<String> = None;
    let mut image_from_rels: Option<String> = None;

    for r in &relations {
        match r.relation_type.as_deref() {
            Some("wikipedia") => {
                if wikipedia_url.is_none() {
                    wikipedia_url = r.url.as_ref().and_then(|u| u.resource.clone());
                }
            }
            Some("wikidata") => {
                if wikidata_url.is_none() {
                    wikidata_url = r.url.as_ref().and_then(|u| u.resource.clone());
                }
            }
            Some("image") if image_from_rels.is_none() => {
                image_from_rels = r.url.as_ref().and_then(|u| u.resource.clone());
            }
            _ => {}
        }
    }

    // 3. Resolve Wikipedia article title
    //    Priority: direct wikipedia URL → wikidata sitelinks → artist name fallback
    let wiki_article: Option<(String, String)> = if let Some(ref url) = wikipedia_url {
        tracing::debug!("found wikipedia relation: {url}");
        parse_wikipedia_url(url)
    } else if let Some(ref wd_url) = wikidata_url {
        tracing::debug!("found wikidata relation: {wd_url}, resolving sitelinks");
        resolve_wikidata_to_wikipedia(client, wd_url).await
    } else {
        tracing::debug!(
            "no wikipedia/wikidata relation, falling back to artist name: {artist_name}"
        );
        // Fallback: use artist name directly (replace spaces with underscores)
        let title = artist_name.replace(' ', "_");
        Some(("en".to_string(), title))
    };

    let mut bio = None;
    let mut image_url = image_from_rels;

    // 4. Fetch Wikipedia summary
    if let Some((lang, title)) = wiki_article {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let summary_url = format!("https://{lang}.wikipedia.org/api/rest_v1/page/summary/{title}");
        tracing::info!("fetching Wikipedia summary: {summary_url}");

        match client.get(&summary_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(summary) = resp.json::<WikipediaSummary>().await {
                    bio = summary.extract;
                    if image_url.is_none() {
                        image_url = summary.thumbnail.and_then(|t| t.source);
                    }
                    tracing::info!(
                        "enriched artist \"{artist_name}\": bio={}chars, image={}",
                        bio.as_ref().map(|b| b.len()).unwrap_or(0),
                        image_url.is_some()
                    );
                }
            }
            Ok(resp) => {
                tracing::debug!("Wikipedia summary returned {}", resp.status());
            }
            Err(e) => {
                tracing::debug!("Wikipedia summary request failed: {e}");
            }
        }
    }

    (bio, image_url)
}

/// Resolve a Wikidata URL (e.g. https://www.wikidata.org/wiki/Q44190) to a
/// Wikipedia (lang, title) pair via the Wikidata API sitelinks.
async fn resolve_wikidata_to_wikipedia(
    client: &Client,
    wikidata_url: &str,
) -> Option<(String, String)> {
    // Extract QID from URL like "https://www.wikidata.org/wiki/Q44190"
    let qid = wikidata_url.rsplit('/').next()?;
    if !qid.starts_with('Q') {
        return None;
    }

    let api_url = format!(
        "https://www.wikidata.org/w/api.php?action=wbgetentities&ids={qid}&props=sitelinks&format=json"
    );

    let resp = client.get(&api_url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let data: WikidataResponse = resp.json().await.ok()?;
    let entities = data.entities?;
    let entity = entities.get(qid)?;
    let sitelinks = entity.sitelinks.as_ref()?;

    // Try English first, then French, then any wiki
    for wiki_key in &["enwiki", "frwiki"] {
        if let Some(sl) = sitelinks.get(*wiki_key) {
            if let Some(ref title) = sl.title {
                let lang = wiki_key.strip_suffix("wiki").unwrap_or("en");
                return Some((lang.to_string(), title.replace(' ', "_")));
            }
        }
    }

    // Fallback: any language wiki
    for (key, sl) in sitelinks.iter() {
        if key.ends_with("wiki") && !key.contains("quote") && !key.contains("source") {
            if let Some(ref title) = sl.title {
                let lang = key.strip_suffix("wiki").unwrap_or("en");
                return Some((lang.to_string(), title.replace(' ', "_")));
            }
        }
    }

    None
}

/// Parse a Wikipedia URL into (language_code, article_title).
/// e.g. "https://en.wikipedia.org/wiki/Daft_Punk" → ("en", "Daft_Punk")
fn parse_wikipedia_url(url: &str) -> Option<(String, String)> {
    let url = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let (domain, path) = url.split_once('/')?;
    let lang = domain.split('.').next()?.to_string();
    let title = path.strip_prefix("wiki/")?.to_string();
    Some((lang, title))
}

// ─── Enrich a single track ──────────────────────────────────────────

/// Enrich a single track with MusicBrainz metadata.
/// Returns a `MetadataResult` describing what was found/updated.
struct MetadataProvider {
    client: Client,
    mb_http: MusicBrainzHttp,
    mb_base_url: String,
    caa_base_url: String,
}
impl MetadataProvider {
    async fn load(db: &DatabaseConnection) -> Result<Self, reqwest::Error> {
        let (mb_base_url, agent, caa_base_url) = load_mb_config(db).await;
        Ok(Self {
            client: build_client_with_ua(&agent)?,
            mb_http: MusicBrainzHttp::new(&agent)?,
            mb_base_url,
            caa_base_url,
        })
    }
}

pub async fn enrich_track(
    db: &DatabaseConnection,
    storage: &dyn StorageBackend,
    track_id: Uuid,
) -> MetadataResult {
    enrich_track_choice(db, storage, track_id, None).await
}

pub async fn enrich_track_choice(
    db: &DatabaseConnection,
    storage: &dyn StorageBackend,
    track_id: Uuid,
    choice: Option<Uuid>,
) -> MetadataResult {
    let provider = MetadataProvider::load(db).await;
    enrich_with_provider(db, storage, track_id, choice, provider.as_ref().ok()).await
}

// Local originals can be enriched even before their first federation projection.
// Withdrawn publications and metadata owned by another catalogue are read-only here.
fn enrichable_tracks() -> sea_orm::Select<track::Entity> {
    track::Entity::find().filter(sea_orm::sea_query::Expr::cust(
        "tracks.file_path <> '' AND tracks.file_path !~ '^[a-zA-Z][a-zA-Z0-9+.-]*:' AND NOT EXISTS (
            SELECT FROM track_publications p LEFT JOIN federation_catalogs c ON c.id=p.catalog_id
            WHERE p.track_id=tracks.id AND (c.is_local IS NOT TRUE OR p.state <> 'active'))",
    ))
}

async fn enrich_with_provider(
    db: &DatabaseConnection,
    storage: &dyn StorageBackend,
    track_id: Uuid,
    choice: Option<Uuid>,
    provider: Option<&MetadataProvider>,
) -> MetadataResult {
    let base_result = MetadataResult {
        candidates: Vec::new(),
        track_id,
        status: MetadataStatus::Error,
        recording_mbid: None,
        corrected_title: None,
        artist_mbid: None,
        artist_name: None,
        album_mbid: None,
        album_title: None,
        genre: None,
        year: None,
        cover_url: None,
    };

    // 1. Load track from DB
    let track_model = match enrichable_tracks()
        .filter(track::Column::Id.eq(track_id))
        .one(db)
        .await
    {
        Ok(Some(t)) => t,
        Ok(None) => {
            return MetadataResult {
                status: MetadataStatus::NotFound,
                ..base_result
            }
        }
        Err(_) => return base_result,
    };

    // An explicit admin choice may replace an existing recording ID.
    let already_enriched_by_mb = track_model.musicbrainz_id.is_some();
    if already_enriched_by_mb && choice.is_none() {
        return MetadataResult {
            status: MetadataStatus::AlreadyEnriched,
            recording_mbid: track_model.musicbrainz_id.clone(),
            ..base_result
        };
    }

    // 2. Get artist name
    let artist_model = match artist::Entity::find_by_id(track_model.artist_id)
        .one(db)
        .await
    {
        Ok(Some(a)) => a,
        _ => return base_result,
    };

    let Some(provider) = provider else {
        return base_result;
    };
    let MetadataProvider {
        client,
        mb_http,
        mb_base_url,
        caa_base_url,
    } = provider;
    let album_model = if let Some(id) = track_model.album_id {
        match album::Entity::find_by_id(id).one(db).await {
            Ok(album) => album,
            Err(_) => return base_result,
        }
    } else {
        None
    };
    let resolution = if let Some(mbid) = choice {
        musicbrainz::get_recording(mb_http, mb_base_url, mbid)
            .await
            .map(|r| {
                r.map(RecordingMatch::Reliable)
                    .unwrap_or(RecordingMatch::Absent)
            })
    } else {
        let (id, isrc) = match hints::load(db, storage, track_id, &track_model.file_path).await {
            Ok(hints) => hints,
            Err(error) => {
                tracing::warn!(%track_id,%error,"failed to read recording identifiers");
                return base_result;
            }
        };
        let duration = track_model.duration_secs;
        musicbrainz::resolve_tagged_recording(
            mb_http,
            mb_base_url,
            id,
            isrc.as_deref(),
            &track_model.title,
            (!artist_model.is_placeholder).then_some(artist_model.name.as_str()),
            album_model.as_ref().map(|a| a.title.as_str()),
            (duration.is_finite() && duration > 0.)
                .then(|| (f64::from(duration) * 1000.).round() as u64),
        )
        .await
    };
    let recording = match resolution {
        Ok(RecordingMatch::Reliable(r)) => r,
        Ok(RecordingMatch::Absent) => {
            return MetadataResult {
                status: MetadataStatus::NotFound,
                ..base_result
            }
        }
        Ok(RecordingMatch::Ambiguous(rows)) => {
            return MetadataResult {
                status: MetadataStatus::Ambiguous,
                candidates: rows.into_iter().take(5).map(Into::into).collect(),
                ..base_result
            }
        }
        Err(error) => {
            tracing::warn!(%track_id,%error,"MusicBrainz lookup unavailable");
            return base_result;
        }
    };
    let recording_mbid = recording.id.clone();
    let corrected_title = Some(recording.title.clone());
    let genre = recording
        .tags
        .iter()
        .max_by_key(|t| t.count)
        .map(|t| t.name.clone());
    let year = recording
        .first_release_date
        .as_deref()
        .and_then(|d| d.split('-').next())
        .and_then(|s| s.parse::<i16>().ok());
    let mb_artist_name = join_credit(&recording);
    // An artist MBID identifies a single performer, not a combined credit.
    let artist_mbid = match recording.artist_credit.as_slice() {
        [entry]
            if artist_model.is_placeholder
                || same_label(&artist_model.name, &entry.artist.name)
                || entry
                    .name
                    .as_deref()
                    .is_some_and(|n| same_label(&artist_model.name, n)) =>
        {
            Some(entry.artist.id.clone())
        }
        _ => None,
    };
    let release = album_model.as_ref().and_then(|a| {
        recording
            .releases
            .iter()
            .find(|r| same_label(&a.title, &r.title))
    });
    let album_mbid = release.map(|r| r.id.clone());
    let album_title = release.map(|r| r.title.clone());
    let tx = match crate::storage_catalog::begin_write(db).await {
        Ok(tx) => tx,
        Err(_) => return base_result,
    };
    let current = match enrichable_tracks()
        .filter(track::Column::Id.eq(track_id))
        .lock_exclusive()
        .one(&tx)
        .await
    {
        Ok(Some(row)) => row,
        _ => return base_result,
    };
    if current.title != track_model.title
        || current.artist_id != track_model.artist_id
        || current.album_id != track_model.album_id
        || current.musicbrainz_id != track_model.musicbrainz_id
        || current.file_hash != track_model.file_hash
        || current.content_hash != track_model.content_hash
    {
        return base_result;
    }
    let mut artist_model = artist_model;
    let fill_credit = artist_model.is_placeholder && mb_artist_name.is_some();
    if fill_credit {
        let id = match crate::storage_catalog::releases::artist_id(&tx, mb_artist_name.as_deref())
            .await
        {
            Ok(id) => id,
            Err(_) => return base_result,
        };
        artist_model = match artist::Entity::find_by_id(id).one(&tx).await {
            Ok(Some(a)) => a,
            _ => return base_result,
        };
    }
    // 5. Update track with MusicBrainz ID and enriched data
    //    Only fill in fields that are currently empty (preserve upload metadata)
    let mut track_update: track::ActiveModel = current.into();
    if fill_credit {
        track_update.artist_id = Set(artist_model.id);
    }
    track_update.musicbrainz_id = Set(Some(recording_mbid.clone()));
    // Only update title if it looks like a filename or is very generic
    if let Some(ref title) = corrected_title {
        let current = &track_model.title;
        let looks_like_filename = current.contains('.')
            && (current.ends_with(".mp3")
                || current.ends_with(".flac")
                || current.ends_with(".ogg")
                || current.ends_with(".wav")
                || current.ends_with(".m4a")
                || current.ends_with(".opus")
                || current.ends_with(".aiff")
                || current.ends_with(".aif"));
        if looks_like_filename {
            track_update.title = Set(title.clone());
        }
    }
    if genre.is_some() && track_model.genre.is_none() {
        track_update.genre = Set(genre.clone());
    }
    if year.is_some() && track_model.year.is_none() {
        track_update.year = Set(year);
    }
    if let Err(e) = track_update.update(&tx).await {
        tracing::warn!(error = %e, track_id = %track_id, "failed to update track metadata");
        return base_result;
    }

    if let Err(error) = tx.commit().await {
        tracing::warn!(%track_id,%error,"failed to commit recording metadata");
        return base_result;
    }
    // 6. Update artist with MusicBrainz ID, bio, and image (only if not already set)
    if !artist_model.is_placeholder
        && artist_model.musicbrainz_id.is_none()
        && artist_mbid.is_some()
    {
        let mut artist_update: artist::ActiveModel = artist_model.clone().into();
        if let Some(ref mbid) = artist_mbid {
            artist_update.musicbrainz_id = Set(Some(mbid.clone()));

            // Fetch bio and image from Wikipedia via MusicBrainz relations
            if artist_model.bio.is_none() || artist_model.image_url.is_none() {
                let (bio, image) =
                    fetch_artist_bio_image(client, mb_http, mbid, &artist_model.name, mb_base_url)
                        .await;
                if artist_model.bio.is_none() {
                    if let Some(ref bio_text) = bio {
                        artist_update.bio = Set(Some(bio_text.clone()));
                    }
                }
                if artist_model.image_url.is_none() {
                    if let Some(ref img) = image {
                        artist_update.image_url = Set(Some(img.clone()));
                    }
                }
            }
        }
        if let Err(e) = artist_update.update(db).await {
            tracing::warn!(error = %e, "failed to update artist metadata");
        }
    }

    // 7. Update album with MusicBrainz ID and cover art
    let mut result_cover_url = None;
    if let Some(album_model) = album_model {
        if album_mbid.is_some()
            && (album_model.musicbrainz_id.is_none() || album_model.musicbrainz_id == album_mbid)
        {
            let mut album_update: album::ActiveModel = album_model.clone().into();

            // Set album MusicBrainz ID
            if album_model.musicbrainz_id.is_none() {
                if let Some(ref mbid) = album_mbid {
                    album_update.musicbrainz_id = Set(Some(mbid.clone()));
                }
            }
            // Only update album title if it looks like a placeholder
            if let Some(ref title) = album_title {
                if album_model.title == "Unknown Album" || album_model.title == "Inconnu" {
                    album_update.title = Set(title.clone());
                }
            }
            if genre.is_some() && album_model.genre.is_none() {
                album_update.genre = Set(genre.clone());
            }
            if year.is_some() && album_model.year.is_none() {
                album_update.year = Set(year);
            }

            // Fetch and store cover art if album has none
            if album_model.cover_url.is_none() {
                if let Some(ref rel_mbid) = album_mbid {
                    if let Some(cover_url) =
                        fetch_cover_art_url(client, rel_mbid, caa_base_url).await
                    {
                        // Download and store locally
                        if let Some(cover_bytes) = download_cover(client, &cover_url).await {
                            // Use the track uploader as owner; fall back to a nil UUID for system-enriched tracks
                            let owner_id = track_model.uploaded_by.unwrap_or_else(Uuid::nil);
                            if let Ok(relative) = storage
                                .store_cover(
                                    owner_id,
                                    Some(album_model.title.as_str()),
                                    &cover_bytes,
                                )
                                .await
                            {
                                let media_url = format!("/api/media/{relative}");
                                album_update.cover_url = Set(Some(media_url.clone()));
                                result_cover_url = Some(media_url);
                            }
                        }
                    }
                }
            } else {
                result_cover_url = album_model.cover_url.clone();
            }

            if let Err(e) = album_update.update(db).await {
                tracing::warn!(error = %e, "failed to update album metadata");
            }
        }
    }

    MetadataResult {
        candidates: Vec::new(),
        track_id,
        status: MetadataStatus::Enriched,
        recording_mbid: Some(recording_mbid),
        corrected_title,
        artist_mbid,
        artist_name: mb_artist_name,
        album_mbid,
        album_title,
        genre,
        year,
        cover_url: result_cover_url,
    }
}

// ─── AI Fallback enrichment ─────────────────────────────────────────

/// Response structure expected from the AI for metadata resolution.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AiMetadataResponse {
    genre: Option<String>,
    year: Option<i16>,
    album: Option<String>,
    corrected_title: Option<String>,
    corrected_artist: Option<String>,
}

/// Check if the AI API is configured (has a non-empty api key).
#[allow(dead_code)]
async fn get_ai_config(db: &DatabaseConnection) -> Option<(String, String, String)> {
    let api_key = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("ai_api_key"))
        .one(db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value)
        .filter(|v| !v.trim().is_empty())?;

    let base_url = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("ai_base_url"))
        .one(db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value)
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

    let model = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("ai_model"))
        .one(db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value)
        .unwrap_or_else(|| "gpt-4o-mini".to_string());

    Some((api_key, base_url, model))
}

/// Try to enrich track metadata using an AI API when MusicBrainz finds nothing.
/// Only fills in fields that are currently empty.
#[allow(dead_code)]
async fn enrich_track_with_ai_fallback(
    db: &DatabaseConnection,
    _client: &Client,
    track_model: &track::Model,
    artist_model: &artist::Model,
    base_result: MetadataResult,
) -> MetadataResult {
    let track_id = track_model.id;

    // Check if AI is configured
    let (api_key, base_url, model) = match get_ai_config(db).await {
        Some(config) => config,
        None => {
            tracing::debug!(track_id = %track_id, "AI not configured, returning NotFound");
            return MetadataResult {
                status: MetadataStatus::NotFound,
                ..base_result
            };
        }
    };

    tracing::info!(
        track_id = %track_id,
        title = %track_model.title,
        artist = %artist_model.name,
        "Attempting AI metadata enrichment"
    );

    let prompt = format!(
        r#"You are a music metadata expert. Given the following track information, provide accurate metadata.

Track title: "{}"
Artist name: "{}"
Audio format: {}
Duration: {:.0} seconds

Respond with a JSON object containing ONLY the fields you are confident about. Do not guess — omit any field you are unsure of.

Fields:
- "genre": the primary music genre (e.g. "Rock", "Hip-Hop", "Electronic", "Classical", "Jazz")
- "year": the release year as a number (e.g. 2019)
- "album": the album name this track belongs to (if known)
- "corrected_title": the corrected/proper title (only if the provided title seems wrong, has typos, or is a filename)
- "corrected_artist": the corrected/proper artist name (only if the provided name seems wrong)

Respond ONLY with valid JSON, no markdown, no explanation."#,
        track_model.title, artist_model.name, track_model.format, track_model.duration_secs,
    );

    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": "You are a music metadata expert. Always respond with valid JSON only." },
            { "role": "user", "content": prompt }
        ],
        "temperature": 0.2,
        "max_tokens": 300,
    });

    let ai_client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return MetadataResult {
                status: MetadataStatus::NotFound,
                ..base_result
            };
        }
    };

    let ai_response = match ai_client
        .post(format!("{base_url}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            tracing::warn!("AI API request failed for track {track_id}: {e}");
            return MetadataResult {
                status: MetadataStatus::NotFound,
                ..base_result
            };
        }
    };

    if !ai_response.status().is_success() {
        let status = ai_response.status();
        tracing::warn!("AI API returned {status} for track {track_id}");
        return MetadataResult {
            status: MetadataStatus::NotFound,
            ..base_result
        };
    }

    let ai_body: serde_json::Value = match ai_response.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Failed to parse AI response for track {track_id}: {e}");
            return MetadataResult {
                status: MetadataStatus::NotFound,
                ..base_result
            };
        }
    };

    let content = ai_body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("{}");

    // Strip markdown code fences if present
    let clean = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let ai_meta: AiMetadataResponse = match serde_json::from_str(clean) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                "Failed to parse AI metadata JSON for track {track_id}: {e} — raw: {clean}"
            );
            return MetadataResult {
                status: MetadataStatus::NotFound,
                ..base_result
            };
        }
    };

    tracing::info!(
        track_id = %track_id,
        genre = ?ai_meta.genre,
        year = ?ai_meta.year,
        album = ?ai_meta.album,
        "AI enrichment result"
    );

    // Only update fields that are currently empty on the track
    let mut track_update: track::ActiveModel = track_model.clone().into();
    let mut any_update = false;

    if track_model.genre.is_none() {
        if let Some(ref genre) = ai_meta.genre {
            track_update.genre = Set(Some(genre.clone()));
            any_update = true;
        }
    }
    if track_model.year.is_none() {
        if let Some(year) = ai_meta.year {
            track_update.year = Set(Some(year));
            any_update = true;
        }
    }
    // Only correct title if it looks like a filename
    if let Some(ref corrected) = ai_meta.corrected_title {
        let current = &track_model.title;
        let looks_like_filename = current.contains('.')
            && (current.ends_with(".mp3")
                || current.ends_with(".flac")
                || current.ends_with(".ogg")
                || current.ends_with(".wav")
                || current.ends_with(".m4a")
                || current.ends_with(".opus")
                || current.ends_with(".aiff")
                || current.ends_with(".aif"));
        if looks_like_filename {
            track_update.title = Set(corrected.clone());
            any_update = true;
        }
    }

    if any_update {
        if let Err(e) = track_update.update(db).await {
            tracing::warn!(error = %e, track_id = %track_id, "failed to update track with AI metadata");
        }
    }

    // Update album title if we found one and the current one is empty/unknown
    if let Some(ref album_name) = ai_meta.album {
        if let Some(album_id) = track_model.album_id {
            if let Ok(Some(album_model)) = album::Entity::find_by_id(album_id).one(db).await {
                if album_model.title == "Unknown Album" || album_model.title == "Inconnu" {
                    let mut album_update: album::ActiveModel = album_model.into();
                    album_update.title = Set(album_name.clone());
                    if let Err(e) = album_update.update(db).await {
                        tracing::warn!(error = %e, "failed to update album title with AI metadata");
                    }
                }
            }
        }
    }

    MetadataResult {
        candidates: Vec::new(),
        track_id,
        status: MetadataStatus::EnrichedByAi,
        recording_mbid: None,
        corrected_title: ai_meta.corrected_title,
        artist_mbid: None,
        artist_name: ai_meta.corrected_artist,
        album_mbid: None,
        album_title: ai_meta.album,
        genre: ai_meta.genre,
        year: ai_meta.year,
        cover_url: None,
    }
}

// ─── Metadata enrichment task tracker ──────────────────────────────

/// Status of the background metadata enrichment task.
///
/// Serialized as a tagged union (`#[serde(tag = "status")]`) so the JSON
/// response always contains a `"status"` field (`"running"`, `"completed"`,
/// or `"error"`) alongside variant-specific data. This lets the frontend
/// discriminate on a single key when polling `/admin/metadata/task-status`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status")]
pub enum MetadataTaskStatus {
    /// Task is currently running — includes live progress counters.
    #[serde(rename = "running")]
    Running { progress: MetadataTaskProgress },
    /// Task completed successfully — includes final summary counters.
    #[serde(rename = "completed")]
    Completed { result: MetadataTaskResult },
    /// Task failed with an unrecoverable error.
    #[serde(rename = "error")]
    Error { message: String },
}

/// Live progress snapshot for a running metadata enrichment task.
///
/// Updated in the tracker before each track is processed, so the admin
/// frontend can display a progress bar and per-category counters.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MetadataTaskProgress {
    /// Number of tracks processed so far (0-indexed at the start of each track).
    pub processed: u64,
    /// Total number of tracks that will be processed in this batch.
    pub total: u64,
    /// Tracks successfully enriched via MusicBrainz (or AI, if enabled).
    pub enriched: u64,
    /// Tracks for which no MusicBrainz match was found.
    pub not_found: u64,
    pub ambiguous: u64,
    /// Tracks that encountered an error during enrichment.
    pub errors: u64,
    /// Title of the track currently being processed, if any.
    pub current_track: Option<String>,
}

/// Final summary returned when a background metadata enrichment task completes.
///
/// Stored in the tracker until the admin dismisses it, so the result can
/// be displayed even after the task has finished.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MetadataTaskResult {
    /// Total number of tracks that were evaluated (including already-enriched ones).
    pub total_processed: u64,
    /// Tracks successfully enriched from MusicBrainz (or AI).
    pub enriched: u64,
    /// Tracks for which no external match was found.
    pub not_found: u64,
    pub ambiguous: u64,
    /// Tracks that failed due to network or DB errors.
    pub errors: u64,
    /// Tracks skipped because they already had a MusicBrainz ID.
    pub already_enriched: u64,
}

/// Shared handle for tracking the metadata enrichment background task.
///
/// Wraps an `Option<MetadataTaskStatus>` behind `Arc<Mutex<>>` so the
/// spawned tokio task can update progress while admin handlers read it
/// concurrently. `None` means no task has been started (or it was dismissed).
///
/// Injected into the Axum router as an `Extension` layer so both the
/// background task and the HTTP handlers share the same instance.
pub type MetadataTaskTrackerHandle = Arc<Mutex<Option<MetadataTaskStatus>>>;

/// Create a new metadata task tracker, initialized to `None` (idle).
///
/// Called once at server startup in `main.rs` and installed as an
/// Axum `Extension` on the admin router.
pub fn new_metadata_tracker() -> MetadataTaskTrackerHandle {
    Arc::new(Mutex::new(None))
}

/// Freeze the import-time cutoff and advance by UUID through bounded pages.
/// A miss stays eligible for the next operator run and cannot loop in this one.
pub async fn enrich_all_tracks_background(
    db: &DatabaseConnection,
    storage: &dyn StorageBackend,
    tracker: &MetadataTaskTrackerHandle,
) -> Result<MetadataTaskResult, DbErr> {
    let cutoff = chrono::Utc::now().fixed_offset();
    let candidates = enrichable_tracks()
        .filter(track::Column::MusicbrainzId.is_null())
        .filter(track::Column::CreatedAt.lte(cutoff));
    let total = candidates.clone().count(db).await?;
    let mut result = MetadataTaskResult::default();
    let provider = MetadataProvider::load(db)
        .await
        .map_err(|e| DbErr::Custom(e.to_string()))?;
    tracing::info!(total, "starting background metadata enrichment");

    let mut after = None;
    loop {
        let mut page = candidates
            .clone()
            .select_only()
            .columns([track::Column::Id, track::Column::Title]);
        if let Some(last) = after {
            page = page.filter(track::Column::Id.gt(last));
        }
        let rows = page
            .order_by_asc(track::Column::Id)
            .limit(64)
            .into_tuple::<(Uuid, String)>()
            .all(db)
            .await?;
        let Some((last, _)) = rows.last() else {
            break;
        };
        after = Some(*last);
        for (id, title) in rows {
            *tracker.lock().await = Some(MetadataTaskStatus::Running {
                progress: MetadataTaskProgress {
                    processed: result.total_processed,
                    total,
                    enriched: result.enriched,
                    not_found: result.not_found,
                    ambiguous: result.ambiguous,
                    errors: result.errors,
                    current_track: Some(title),
                },
            });

            match enrich_with_provider(db, storage, id, None, Some(&provider))
                .await
                .status
            {
                MetadataStatus::Enriched | MetadataStatus::EnrichedByAi => result.enriched += 1,
                MetadataStatus::NotFound => result.not_found += 1,
                MetadataStatus::Ambiguous => result.ambiguous += 1,
                MetadataStatus::Error => result.errors += 1,
                MetadataStatus::AlreadyEnriched => result.already_enriched += 1,
            }
            result.total_processed += 1;
        }
    }

    tracing::info!(
        total_processed = result.total_processed,
        enriched = result.enriched,
        not_found = result.not_found,
        errors = result.errors,
        "background metadata enrichment completed"
    );
    Ok(result)
}

// ─── Instance health check ──────────────────────────────────────────

/// Check if a remote instance is reachable by hitting its NodeInfo endpoint.
pub async fn check_instance_health(domain: &str) -> bool {
    let client = match build_client() {
        Ok(c) => c,
        Err(_) => return false,
    };

    let base = instance_base_url(domain);
    let url = format!("{base}/.well-known/nodeinfo");
    match client.get(&url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => {
            // Try HTTP as fallback (when scheme is https)
            if base.starts_with("https") {
                let url_http = format!("http://{domain}/.well-known/nodeinfo");
                match client.get(&url_http).send().await {
                    Ok(resp) => resp.status().is_success(),
                    Err(_) => false,
                }
            } else {
                false
            }
        }
    }
}

/// Check availability of all known instances and update remote_tracks accordingly.
/// P2P tracks (instance_domain starting with "p2p://") are skipped —
/// their availability is managed by the P2P track health monitor.
pub async fn refresh_instance_availability(db: &DatabaseConnection) {
    use soundtime_db::entities::remote_track;

    // Get distinct instance domains
    let remote_tracks = remote_track::Entity::find()
        .all(db)
        .await
        .unwrap_or_default();

    let mut domains: std::collections::HashSet<String> = std::collections::HashSet::new();
    for rt in &remote_tracks {
        // Skip P2P tracks — they use iroh, not HTTP NodeInfo
        if rt.instance_domain.starts_with("p2p://") {
            continue;
        }
        domains.insert(rt.instance_domain.clone());
    }

    let mut availability: std::collections::HashMap<String, bool> =
        std::collections::HashMap::new();

    for domain in &domains {
        let is_available = check_instance_health(domain).await;
        availability.insert(domain.clone(), is_available);
        // Small delay to avoid hammering
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    // Update remote_tracks availability (skip P2P tracks)
    for rt in remote_tracks {
        if rt.instance_domain.starts_with("p2p://") {
            continue;
        }
        let is_available = availability
            .get(&rt.instance_domain)
            .copied()
            .unwrap_or(false);
        if rt.is_available != is_available {
            let mut update: remote_track::ActiveModel = rt.into();
            update.is_available = Set(is_available);
            update.last_checked_at = Set(Some(chrono::Utc::now().into()));
            if let Err(e) = update.update(db).await {
                tracing::warn!(error = %e, "failed to update remote track availability");
            }
        }
    }
}

// ─── Best bitrate resolution ────────────────────────────────────────

/// Information about the best available version of a track.
#[derive(Debug, Clone, Serialize)]
pub struct BestTrackSource {
    /// "local" or the remote instance domain
    pub source: String,
    /// Stream URL (local path or remote URL)
    pub stream_url: String,
    /// The best bitrate found
    pub bitrate: Option<i32>,
    /// Audio format
    pub format: Option<String>,
    /// Whether this source is currently available
    pub is_available: bool,
}

/// Find the best available version of a track (local or remote).
/// Prefers highest bitrate among available sources.
pub async fn resolve_best_source(
    db: &DatabaseConnection,
    track_id: Uuid,
    domain: &str,
) -> Option<BestTrackSource> {
    use soundtime_db::entities::remote_track;

    // Local track info
    let local = track::Entity::find_by_id(track_id).one(db).await.ok()??;

    let local_source = BestTrackSource {
        source: "local".to_string(),
        stream_url: format!("{}/api/tracks/{track_id}/stream", instance_base_url(domain)),
        bitrate: local.bitrate,
        format: Some(local.format.clone()),
        is_available: true,
    };

    // Find remote alternatives (matched by musicbrainz_id OR local_track_id)
    let mut remote_sources: Vec<BestTrackSource> = Vec::new();

    // By local_track_id reference
    let remotes_by_id = remote_track::Entity::find()
        .filter(remote_track::Column::LocalTrackId.eq(Some(track_id)))
        .filter(remote_track::Column::IsAvailable.eq(true))
        .all(db)
        .await
        .unwrap_or_default();

    for rt in remotes_by_id {
        remote_sources.push(BestTrackSource {
            source: rt.instance_domain,
            stream_url: rt.remote_stream_url,
            bitrate: rt.bitrate,
            format: rt.format,
            is_available: rt.is_available,
        });
    }

    // Also search by musicbrainz_id if the local track has one
    if let Some(ref mbid) = local.musicbrainz_id {
        let remotes_by_mbid = remote_track::Entity::find()
            .filter(remote_track::Column::MusicbrainzId.eq(mbid.clone()))
            .filter(remote_track::Column::IsAvailable.eq(true))
            .filter(remote_track::Column::LocalTrackId.ne(Some(track_id)))
            .all(db)
            .await
            .unwrap_or_default();

        for rt in remotes_by_mbid {
            remote_sources.push(BestTrackSource {
                source: rt.instance_domain,
                stream_url: rt.remote_stream_url,
                bitrate: rt.bitrate,
                format: rt.format,
                is_available: rt.is_available,
            });
        }
    }

    // Find the best bitrate among all available sources
    let mut best = local_source;
    for rs in remote_sources {
        if rs.is_available {
            let rs_bitrate = rs.bitrate.unwrap_or(0);
            let best_bitrate = best.bitrate.unwrap_or(0);
            if rs_bitrate > best_bitrate {
                best = rs;
            }
        }
    }

    Some(best)
}

/// Register a remote track discovered via federation.
/// Automatically deduplicates by matching title+artist or musicbrainz_id.
#[allow(clippy::too_many_arguments)]
pub async fn register_remote_track(
    db: &DatabaseConnection,
    title: &str,
    artist_name: &str,
    album_title: Option<&str>,
    instance_domain: &str,
    remote_uri: &str,
    remote_stream_url: &str,
    bitrate: Option<i32>,
    sample_rate: Option<i32>,
    format: Option<&str>,
    musicbrainz_id: Option<&str>,
) -> Result<Uuid, String> {
    use soundtime_db::entities::remote_track;

    // Check if this remote URI already exists
    let existing = remote_track::Entity::find()
        .filter(remote_track::Column::RemoteUri.eq(remote_uri))
        .one(db)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    if let Some(existing) = existing {
        // Update bitrate if better
        if bitrate.unwrap_or(0) > existing.bitrate.unwrap_or(0) {
            let mut update: remote_track::ActiveModel = existing.clone().into();
            update.bitrate = Set(bitrate);
            update.sample_rate = Set(sample_rate);
            update.format = Set(format.map(|f| f.to_string()));
            update.is_available = Set(true);
            update.last_checked_at = Set(Some(chrono::Utc::now().into()));
            if let Err(e) = update.update(db).await {
                tracing::warn!(error = %e, "failed to update remote track bitrate");
            }
        }
        return Ok(existing.id);
    }

    // Try to match to a local track
    let local_track_id = find_matching_local_track(db, title, artist_name, musicbrainz_id).await;

    let id = Uuid::new_v4();
    remote_track::ActiveModel {
        id: Set(id),
        local_track_id: Set(local_track_id),
        musicbrainz_id: Set(musicbrainz_id.map(|s| s.to_string())),
        title: Set(title.to_string()),
        artist_name: Set(artist_name.to_string()),
        album_title: Set(album_title.map(|s| s.to_string())),
        instance_domain: Set(instance_domain.to_string()),
        remote_uri: Set(remote_uri.to_string()),
        remote_stream_url: Set(remote_stream_url.to_string()),
        bitrate: Set(bitrate),
        sample_rate: Set(sample_rate),
        format: Set(format.map(|f| f.to_string())),
        is_available: Set(true),
        last_checked_at: Set(Some(chrono::Utc::now().into())),
        created_at: Set(chrono::Utc::now().into()),
    }
    .insert(db)
    .await
    .map_err(|e| format!("Insert remote track: {e}"))?;

    Ok(id)
}

/// Find a local track matching the given criteria.
async fn find_matching_local_track(
    db: &DatabaseConnection,
    title: &str,
    artist_name: &str,
    musicbrainz_id: Option<&str>,
) -> Option<Uuid> {
    // First try by MusicBrainz ID (most reliable)
    if let Some(mbid) = musicbrainz_id {
        if let Ok(Some(t)) = track::Entity::find()
            .filter(track::Column::MusicbrainzId.eq(mbid))
            .one(db)
            .await
        {
            return Some(t.id);
        }
    }

    // A remote recording's explicit credit can only match a credited artist.
    let artists = artist::Entity::credited()
        .filter(artist::Column::Name.eq(artist_name))
        .all(db)
        .await
        .unwrap_or_default();

    for a in artists {
        if let Ok(Some(t)) = track::Entity::find()
            .filter(track::Column::Title.eq(title))
            .filter(track::Column::ArtistId.eq(a.id))
            .one(db)
            .await
        {
            return Some(t.id);
        }
    }

    None
}
