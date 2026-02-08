//! MusicBrainz API client for enriching track metadata.
//!
//! Uses the MusicBrainz Web Service v2 to look up recordings by title+artist
//! and retrieve canonical metadata (MusicBrainz ID, genre, year, etc.).
//!
//! Rate-limited to 1 request/second per MusicBrainz API terms.

use serde::Deserialize;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, warn};

/// MusicBrainz API base URL.
const MB_BASE_URL: &str = "https://musicbrainz.org/ws/2";

/// User-Agent required by MusicBrainz API policy.
const MB_USER_AGENT: &str = "SoundTime/0.1.0 (https://github.com/CICCADA-CORP/SoundTime)";

/// Rate-limit: max 1 concurrent request (MusicBrainz enforces 1 req/s).
static MB_SEMAPHORE: Semaphore = Semaphore::const_new(1);

/// A resolved recording from MusicBrainz.
#[derive(Debug, Clone)]
pub struct MusicBrainzRecording {
    pub id: String,
    pub title: String,
    pub artist_name: Option<String>,
    pub release_title: Option<String>,
    pub year: Option<i16>,
    pub score: u8,
}

// ── Internal API response types ─────────────────────────────────

#[derive(Deserialize)]
struct MbSearchResponse {
    recordings: Vec<MbRecording>,
}

#[derive(Deserialize)]
struct MbRecording {
    id: String,
    title: String,
    score: Option<u8>,
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<MbArtistCredit>>,
    releases: Option<Vec<MbRelease>>,
}

#[derive(Deserialize)]
struct MbArtistCredit {
    artist: MbArtist,
}

#[derive(Deserialize)]
struct MbArtist {
    name: String,
}

#[derive(Deserialize)]
struct MbRelease {
    title: String,
    date: Option<String>,
}

/// MusicBrainz client for metadata resolution.
pub struct MusicBrainzClient {
    http: reqwest::Client,
}

impl MusicBrainzClient {
    /// Create a new MusicBrainz client with the proper User-Agent.
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(MB_USER_AGENT)
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        Self { http }
    }

    /// Look up a recording by title and artist name.
    /// Returns the best match if found with a score >= 80.
    pub async fn lookup_recording(
        &self,
        title: &str,
        artist: &str,
    ) -> Option<MusicBrainzRecording> {
        // Acquire rate-limit permit
        let _permit = MB_SEMAPHORE.acquire().await.ok()?;

        let query = format!(
            "recording:\"{}\" AND artist:\"{}\"",
            title.replace('"', ""),
            artist.replace('"', "")
        );

        let url = format!(
            "{MB_BASE_URL}/recording?query={}&fmt=json&limit=3",
            urlencoding::encode(&query)
        );

        debug!(title = title, artist = artist, "querying MusicBrainz");

        let resp = match self.http.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("MusicBrainz request failed: {e}");
                // Respect rate limit even on failure
                tokio::time::sleep(Duration::from_secs(1)).await;
                return None;
            }
        };

        // Respect rate limit
        tokio::time::sleep(Duration::from_secs(1)).await;

        if !resp.status().is_success() {
            warn!(status = %resp.status(), "MusicBrainz returned error");
            return None;
        }

        let body: MbSearchResponse = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                warn!("failed to parse MusicBrainz response: {e}");
                return None;
            }
        };

        // Find best match with score >= 80
        body.recordings
            .into_iter()
            .filter(|r| r.score.unwrap_or(0) >= 80)
            .map(|r| {
                let artist_name = r
                    .artist_credit
                    .as_ref()
                    .and_then(|ac| ac.first())
                    .map(|ac| ac.artist.name.clone());

                let (release_title, year) =
                    r.releases
                        .as_ref()
                        .and_then(|rels| rels.first())
                        .map(|rel| {
                            let y = rel.date.as_ref().and_then(|d| {
                                d.split('-').next().and_then(|y| y.parse::<i16>().ok())
                            });
                            (Some(rel.title.clone()), y)
                        })
                        .unwrap_or((None, None));

                MusicBrainzRecording {
                    id: r.id,
                    title: r.title,
                    artist_name,
                    release_title,
                    year,
                    score: r.score.unwrap_or(0),
                }
            })
            .next()
    }
}

impl Default for MusicBrainzClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = MusicBrainzClient::new();
        // Just ensure it doesn't panic
        drop(client);
    }
}
