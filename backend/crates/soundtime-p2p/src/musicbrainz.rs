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
    base_url: String,
}

impl MusicBrainzClient {
    /// Create a new MusicBrainz client with the proper User-Agent.
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(MB_USER_AGENT)
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        Self {
            http,
            base_url: MB_BASE_URL.to_string(),
        }
    }

    /// Create a client pointing at a custom base URL (for testing).
    #[cfg(test)]
    pub(crate) fn with_base_url(base_url: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("failed to build HTTP client");
        Self {
            http,
            base_url: base_url.to_string(),
        }
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
            "{}/recording?query={}&fmt=json&limit=3",
            self.base_url,
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
    use wiremock::matchers::{method, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn test_client_creation() {
        let client = MusicBrainzClient::new();
        assert_eq!(client.base_url, MB_BASE_URL);
        drop(client);
    }

    #[test]
    fn test_client_default() {
        let client = MusicBrainzClient::default();
        assert_eq!(client.base_url, MB_BASE_URL);
    }

    #[test]
    fn test_client_with_base_url() {
        let client = MusicBrainzClient::with_base_url("http://localhost:9999");
        assert_eq!(client.base_url, "http://localhost:9999");
    }

    // ── MusicBrainzRecording fields ──────────────────────────────────

    #[test]
    fn test_recording_debug_clone() {
        let rec = MusicBrainzRecording {
            id: "mb-1".into(),
            title: "Test".into(),
            artist_name: Some("Artist".into()),
            release_title: Some("Album".into()),
            year: Some(2024),
            score: 95,
        };
        let cloned = rec.clone();
        assert_eq!(rec.id, cloned.id);
        assert_eq!(rec.score, cloned.score);
        let debug = format!("{:?}", rec);
        assert!(debug.contains("MusicBrainzRecording"));
    }

    #[test]
    fn test_recording_all_none_optionals() {
        let rec = MusicBrainzRecording {
            id: "mb-2".into(),
            title: "T".into(),
            artist_name: None,
            release_title: None,
            year: None,
            score: 80,
        };
        assert!(rec.artist_name.is_none());
        assert!(rec.release_title.is_none());
        assert!(rec.year.is_none());
    }

    // ── lookup_recording with mock server ────────────────────────────

    fn mb_response_json(score: u8, date: Option<&str>, has_artist: bool, has_release: bool) -> String {
        let artist_credit = if has_artist {
            r#"[{"artist": {"name": "Queen"}}]"#
        } else {
            "null"
        };

        let releases = if has_release {
            let date_field = match date {
                Some(d) => format!(r#""date": "{d}""#),
                None => r#""date": null"#.to_string(),
            };
            format!(r#"[{{"title": "A Night at the Opera", {date_field}}}]"#)
        } else {
            "null".to_string()
        };

        format!(
            r#"{{
                "recordings": [{{
                    "id": "test-mbid-123",
                    "title": "Bohemian Rhapsody",
                    "score": {score},
                    "artist-credit": {artist_credit},
                    "releases": {releases}
                }}]
            }}"#
        )
    }

    #[tokio::test]
    async fn test_lookup_recording_success() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/recording.*"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(mb_response_json(95, Some("1975-10-31"), true, true)),
            )
            .mount(&server)
            .await;

        let client = MusicBrainzClient::with_base_url(&format!("{}/ws/2", server.uri()));
        let result = client.lookup_recording("Bohemian Rhapsody", "Queen").await;

        let rec = result.expect("should find recording");
        assert_eq!(rec.id, "test-mbid-123");
        assert_eq!(rec.title, "Bohemian Rhapsody");
        assert_eq!(rec.artist_name, Some("Queen".to_string()));
        assert_eq!(rec.release_title, Some("A Night at the Opera".to_string()));
        assert_eq!(rec.year, Some(1975));
        assert!(rec.score >= 80);
    }

    #[tokio::test]
    async fn test_lookup_recording_low_score() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/recording.*"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(mb_response_json(50, Some("2000"), true, true)),
            )
            .mount(&server)
            .await;

        let client = MusicBrainzClient::with_base_url(&format!("{}/ws/2", server.uri()));
        let result = client.lookup_recording("Test", "Artist").await;
        assert!(result.is_none(), "score < 80 should return None");
    }

    #[tokio::test]
    async fn test_lookup_recording_no_artist_credit() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/recording.*"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(mb_response_json(90, Some("2020"), false, true)),
            )
            .mount(&server)
            .await;

        let client = MusicBrainzClient::with_base_url(&format!("{}/ws/2", server.uri()));
        let result = client.lookup_recording("Test", "Artist").await;
        let rec = result.unwrap();
        assert!(rec.artist_name.is_none());
    }

    #[tokio::test]
    async fn test_lookup_recording_no_releases() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/recording.*"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(mb_response_json(85, None, true, false)),
            )
            .mount(&server)
            .await;

        let client = MusicBrainzClient::with_base_url(&format!("{}/ws/2", server.uri()));
        let result = client.lookup_recording("Test", "Artist").await;
        let rec = result.unwrap();
        assert!(rec.release_title.is_none());
        assert!(rec.year.is_none());
    }

    #[tokio::test]
    async fn test_lookup_recording_year_only_date() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/recording.*"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(mb_response_json(90, Some("1999"), true, true)),
            )
            .mount(&server)
            .await;

        let client = MusicBrainzClient::with_base_url(&format!("{}/ws/2", server.uri()));
        let result = client.lookup_recording("Test", "Artist").await;
        let rec = result.unwrap();
        assert_eq!(rec.year, Some(1999));
    }

    #[tokio::test]
    async fn test_lookup_recording_http_error_status() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/recording.*"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let client = MusicBrainzClient::with_base_url(&format!("{}/ws/2", server.uri()));
        let result = client.lookup_recording("Test", "Artist").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_lookup_recording_invalid_json_response() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/recording.*"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("not valid json"),
            )
            .mount(&server)
            .await;

        let client = MusicBrainzClient::with_base_url(&format!("{}/ws/2", server.uri()));
        let result = client.lookup_recording("Test", "Artist").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_lookup_recording_empty_recordings() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/recording.*"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"recordings": []}"#),
            )
            .mount(&server)
            .await;

        let client = MusicBrainzClient::with_base_url(&format!("{}/ws/2", server.uri()));
        let result = client.lookup_recording("Test", "Artist").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_lookup_recording_strips_quotes() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/recording.*"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(mb_response_json(90, Some("2020"), true, true)),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = MusicBrainzClient::with_base_url(&format!("{}/ws/2", server.uri()));
        // Quotes in title/artist should be stripped
        let result = client
            .lookup_recording(r#"Test "Title""#, r#"Art"ist"#)
            .await;
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_lookup_recording_release_no_date() {
        let server = MockServer::start().await;

        let body = r#"{
            "recordings": [{
                "id": "x",
                "title": "T",
                "score": 85,
                "artist-credit": [{"artist": {"name": "A"}}],
                "releases": [{"title": "Album", "date": null}]
            }]
        }"#;

        Mock::given(method("GET"))
            .and(path_regex(r"/recording.*"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let client = MusicBrainzClient::with_base_url(&format!("{}/ws/2", server.uri()));
        let result = client.lookup_recording("T", "A").await;
        let rec = result.unwrap();
        assert_eq!(rec.release_title, Some("Album".to_string()));
        assert!(rec.year.is_none());
    }

    #[tokio::test]
    async fn test_lookup_recording_no_score_field() {
        let server = MockServer::start().await;

        let body = r#"{
            "recordings": [{
                "id": "x",
                "title": "T",
                "artist-credit": [{"artist": {"name": "A"}}],
                "releases": [{"title": "Album", "date": "2020"}]
            }]
        }"#;

        Mock::given(method("GET"))
            .and(path_regex(r"/recording.*"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let client = MusicBrainzClient::with_base_url(&format!("{}/ws/2", server.uri()));
        let result = client.lookup_recording("T", "A").await;
        // score defaults to 0, which is < 80 → None
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_lookup_recording_multiple_recordings_picks_first_high_score() {
        let server = MockServer::start().await;

        let body = r#"{
            "recordings": [
                {
                    "id": "low-score",
                    "title": "Wrong",
                    "score": 50,
                    "artist-credit": [{"artist": {"name": "X"}}],
                    "releases": []
                },
                {
                    "id": "high-score",
                    "title": "Right",
                    "score": 95,
                    "artist-credit": [{"artist": {"name": "Y"}}],
                    "releases": [{"title": "Album", "date": "2023"}]
                }
            ]
        }"#;

        Mock::given(method("GET"))
            .and(path_regex(r"/recording.*"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let client = MusicBrainzClient::with_base_url(&format!("{}/ws/2", server.uri()));
        let result = client.lookup_recording("T", "A").await;
        let rec = result.unwrap();
        assert_eq!(rec.id, "high-score");
        assert_eq!(rec.title, "Right");
    }

    #[tokio::test]
    async fn test_lookup_recording_connection_failure() {
        // Point to a non-existent server
        let client = MusicBrainzClient::with_base_url("http://127.0.0.1:1");
        let result = client.lookup_recording("Test", "Artist").await;
        assert!(result.is_none());
    }

    // ── Internal deserialization structs ──────────────────────────────

    #[test]
    fn test_mb_search_response_deserialize() {
        let json = r#"{"recordings": []}"#;
        let resp: MbSearchResponse = serde_json::from_str(json).unwrap();
        assert!(resp.recordings.is_empty());
    }

    #[test]
    fn test_mb_recording_deserialize_minimal() {
        let json = r#"{"id": "abc", "title": "Test"}"#;
        let rec: MbRecording = serde_json::from_str(json).unwrap();
        assert_eq!(rec.id, "abc");
        assert_eq!(rec.title, "Test");
        assert!(rec.score.is_none());
        assert!(rec.artist_credit.is_none());
        assert!(rec.releases.is_none());
    }

    #[test]
    fn test_mb_recording_deserialize_full() {
        let json = r#"{
            "id": "x",
            "title": "T",
            "score": 90,
            "artist-credit": [{"artist": {"name": "A"}}],
            "releases": [{"title": "R", "date": "2020-01-15"}]
        }"#;
        let rec: MbRecording = serde_json::from_str(json).unwrap();
        assert_eq!(rec.score, Some(90));
        let ac = rec.artist_credit.unwrap();
        assert_eq!(ac[0].artist.name, "A");
        let rels = rec.releases.unwrap();
        assert_eq!(rels[0].title, "R");
        assert_eq!(rels[0].date, Some("2020-01-15".to_string()));
    }
}
