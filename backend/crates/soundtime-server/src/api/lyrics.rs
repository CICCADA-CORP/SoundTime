use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use soundtime_db::entities::instance_setting;
use soundtime_db::AppState;
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct LyricsResponse {
    pub lyrics: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MusixmatchResponse {
    message: MusixmatchMessage,
}

#[derive(Debug, Deserialize)]
struct MusixmatchMessage {
    body: Option<MusixmatchBody>,
    header: MusixmatchHeader,
}

#[derive(Debug, Deserialize)]
struct MusixmatchHeader {
    status_code: u32,
}

#[derive(Debug, Deserialize)]
struct MusixmatchBody {
    lyrics: Option<MusixmatchLyrics>,
}

#[derive(Debug, Deserialize)]
struct MusixmatchLyrics {
    lyrics_body: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LyricsComResponse {
    #[serde(default)]
    lyric: Option<String>,
    #[serde(default)]
    err: Option<String>,
}

/// GET /api/tracks/{id}/lyrics — Fetch lyrics on demand
pub async fn get_track_lyrics(
    State(state): State<Arc<AppState>>,
    Path(track_id): Path<uuid::Uuid>,
) -> Result<Json<LyricsResponse>, (StatusCode, Json<serde_json::Value>)> {
    use soundtime_db::entities::track;

    // Get track info (title + artist)
    let track_row = track::Entity::find_by_id(track_id)
        .one(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Track not found" })),
            )
        })?;

    // Get artist name
    use soundtime_db::entities::artist;
    let artist_name = artist::Entity::find_by_id(track_row.artist_id)
        .one(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?
        .map(|a| a.name)
        .unwrap_or_else(|| "Unknown".to_string());

    // Read lyrics settings from instance_settings
    let settings = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.is_in([
            "lyrics_provider",
            "lyrics_musixmatch_key",
            "lyrics_lyricscom_key",
        ]))
        .all(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    let get_setting = |key: &str| -> Option<String> {
        settings
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.value.clone())
            .filter(|v| !v.is_empty())
    };

    let provider = get_setting("lyrics_provider").unwrap_or_default();

    if provider.is_empty() || provider == "none" {
        return Ok(Json(LyricsResponse {
            lyrics: None,
            source: None,
        }));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("HTTP client: {e}") })),
            )
        })?;

    match provider.as_str() {
        "musixmatch" => {
            let api_key = get_setting("lyrics_musixmatch_key").ok_or_else(|| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({ "error": "Musixmatch API key not configured" })),
                )
            })?;

            fetch_musixmatch_lyrics(&client, &api_key, &track_row.title, &artist_name).await
        }
        "lyricscom" => {
            let api_key = get_setting("lyrics_lyricscom_key").ok_or_else(|| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({ "error": "Lyrics.com API key not configured" })),
                )
            })?;

            fetch_lyricscom_lyrics(&client, &api_key, &track_row.title, &artist_name).await
        }
        other => {
            tracing::warn!(provider = other, "unknown lyrics provider");
            Ok(Json(LyricsResponse {
                lyrics: None,
                source: None,
            }))
        }
    }
}

async fn fetch_musixmatch_lyrics(
    client: &reqwest::Client,
    api_key: &str,
    title: &str,
    artist: &str,
) -> Result<Json<LyricsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let url = format!(
        "https://api.musixmatch.com/ws/1.1/matcher.lyrics.get?q_track={}&q_artist={}&apikey={}",
        urlencoding::encode(title),
        urlencoding::encode(artist),
        urlencoding::encode(api_key),
    );

    let resp = client.get(&url).send().await.map_err(|e| {
        tracing::error!(error = %e, "Musixmatch API request failed");
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": "Lyrics API request failed" })),
        )
    })?;

    let data: MusixmatchResponse = resp.json().await.map_err(|e| {
        tracing::error!(error = %e, "Failed to parse Musixmatch response");
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": "Invalid response from lyrics API" })),
        )
    })?;

    if data.message.header.status_code != 200 {
        return Ok(Json(LyricsResponse {
            lyrics: None,
            source: Some("musixmatch".to_string()),
        }));
    }

    let lyrics = data
        .message
        .body
        .and_then(|b| b.lyrics)
        .and_then(|l| l.lyrics_body)
        .filter(|l| !l.is_empty());

    Ok(Json(LyricsResponse {
        lyrics,
        source: Some("musixmatch".to_string()),
    }))
}

async fn fetch_lyricscom_lyrics(
    client: &reqwest::Client,
    api_key: &str,
    title: &str,
    artist: &str,
) -> Result<Json<LyricsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let url = format!(
        "https://www.stands4.com/services/v2/lyrics.php?uid=1&tokenid={}&term={}&artist={}&format=json",
        urlencoding::encode(api_key),
        urlencoding::encode(title),
        urlencoding::encode(artist),
    );

    let resp = client.get(&url).send().await.map_err(|e| {
        tracing::error!(error = %e, "Lyrics.com API request failed");
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": "Lyrics API request failed" })),
        )
    })?;

    let data: LyricsComResponse = resp.json().await.map_err(|e| {
        tracing::error!(error = %e, "Failed to parse Lyrics.com response");
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": "Invalid response from lyrics API" })),
        )
    })?;

    if data.err.is_some() {
        return Ok(Json(LyricsResponse {
            lyrics: None,
            source: Some("lyricscom".to_string()),
        }));
    }

    let lyrics = data.lyric.filter(|l| !l.is_empty());

    Ok(Json(LyricsResponse {
        lyrics,
        source: Some("lyricscom".to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_lyrics_response_with_lyrics() {
        let resp = LyricsResponse {
            lyrics: Some("Hello, world!".to_string()),
            source: Some("musixmatch".to_string()),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["lyrics"], "Hello, world!");
        assert_eq!(val["source"], "musixmatch");
    }

    #[test]
    fn test_serialize_lyrics_response_empty() {
        let resp = LyricsResponse {
            lyrics: None,
            source: None,
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert!(val["lyrics"].is_null());
        assert!(val["source"].is_null());
    }

    #[test]
    fn test_deserialize_musixmatch_response() {
        let json = r#"{
            "message": {
                "header": {"status_code": 200},
                "body": {
                    "lyrics": {
                        "lyrics_body": "Some lyrics here"
                    }
                }
            }
        }"#;
        let resp: MusixmatchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.message.header.status_code, 200);
        assert_eq!(
            resp.message
                .body
                .unwrap()
                .lyrics
                .unwrap()
                .lyrics_body
                .unwrap(),
            "Some lyrics here"
        );
    }

    #[test]
    fn test_deserialize_musixmatch_response_no_lyrics() {
        let json = r#"{
            "message": {
                "header": {"status_code": 404},
                "body": null
            }
        }"#;
        let resp: MusixmatchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.message.header.status_code, 404);
        assert!(resp.message.body.is_none());
    }

    #[test]
    fn test_deserialize_lyricscom_response_with_lyrics() {
        let json = r#"{"lyric": "Hello world", "err": null}"#;
        let resp: LyricsComResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.lyric, Some("Hello world".to_string()));
        assert!(resp.err.is_none());
    }

    #[test]
    fn test_deserialize_lyricscom_response_with_error() {
        let json = r#"{"lyric": null, "err": "not found"}"#;
        let resp: LyricsComResponse = serde_json::from_str(json).unwrap();
        assert!(resp.lyric.is_none());
        assert_eq!(resp.err, Some("not found".to_string()));
    }

    #[test]
    fn test_deserialize_lyricscom_response_empty() {
        let json = r#"{}"#;
        let resp: LyricsComResponse = serde_json::from_str(json).unwrap();
        assert!(resp.lyric.is_none());
        assert!(resp.err.is_none());
    }
}
