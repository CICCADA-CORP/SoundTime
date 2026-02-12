//! AI-powered editorial playlist generation
//!
//! Uses an OpenAI-compatible API to create themed playlists from available tracks.
//! Settings (stored in `instance_settings`):
//!   - `ai_api_key`      — Bearer token for the AI API
//!   - `ai_base_url`     — Base URL (default: https://api.openai.com/v1)
//!   - `ai_model`        — Model name (default: gpt-4o-mini)
//!   - `editorial_last_generated` — ISO 8601 timestamp of last generation

use axum::{extract::State, http::StatusCode, Json};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use soundtime_db::entities::{album, artist, instance_setting, playlist, playlist_track, track};
use soundtime_db::AppState;

// ─── Public endpoint ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct EditorialPlaylistResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub cover_url: Option<String>,
    pub track_count: usize,
    pub tracks: Vec<super::tracks::TrackResponse>,
}

/// GET /api/editorial-playlists — public, returns editorial playlists if AI is configured
pub async fn list_editorial_playlists(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<EditorialPlaylistResponse>>, (StatusCode, String)> {
    // Check if AI is configured
    let api_key = get_setting(&state, "ai_api_key").await;
    if api_key.is_none() || api_key.as_deref() == Some("") {
        return Ok(Json(vec![]));
    }

    let playlists = playlist::Entity::find()
        .filter(playlist::Column::IsEditorial.eq(true))
        .filter(playlist::Column::IsPublic.eq(true))
        .order_by_desc(playlist::Column::UpdatedAt)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let mut results = Vec::new();
    for p in playlists {
        let pt_entries = playlist_track::Entity::find()
            .filter(playlist_track::Column::PlaylistId.eq(p.id))
            .order_by_asc(playlist_track::Column::Position)
            .all(&state.db)
            .await
            .unwrap_or_default();

        let track_ids: Vec<Uuid> = pt_entries.iter().map(|pt| pt.track_id).collect();
        let track_models = if track_ids.is_empty() {
            vec![]
        } else {
            track::Entity::find()
                .filter(track::Column::Id.is_in(track_ids.clone()))
                .all(&state.db)
                .await
                .unwrap_or_default()
        };

        // Reorder by position
        let mut ordered: Vec<super::tracks::TrackResponse> = Vec::new();
        for tid in &track_ids {
            if let Some(t) = track_models.iter().find(|t| &t.id == tid) {
                ordered.push(super::tracks::TrackResponse::from(t.clone()));
            }
        }

        results.push(EditorialPlaylistResponse {
            id: p.id,
            name: p.name,
            description: p.description,
            cover_url: p.cover_url,
            track_count: ordered.len(),
            tracks: ordered,
        });
    }

    Ok(Json(results))
}

// ─── Admin: generate editorial playlists ────────────────────────────

#[derive(Debug, Serialize)]
pub struct GenerateResult {
    pub playlists_created: usize,
    pub message: String,
}

/// POST /api/admin/editorial/generate — generate editorial playlists using AI
pub async fn generate_editorial_playlists(
    State(state): State<Arc<AppState>>,
) -> Result<Json<GenerateResult>, (StatusCode, Json<serde_json::Value>)> {
    let api_key = get_setting(&state, "ai_api_key").await.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "AI API key not configured. Set 'ai_api_key' in settings." })),
        )
    })?;

    if api_key.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "AI API key is empty" })),
        ));
    }

    let base_url = get_setting(&state, "ai_base_url")
        .await
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let model = get_setting(&state, "ai_model")
        .await
        .unwrap_or_else(|| "gpt-4o-mini".to_string());

    // 1. Fetch all available tracks with artist names
    let all_tracks = track::Entity::find()
        .order_by_asc(track::Column::Title)
        .all(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    if all_tracks.len() < 5 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "error": "Not enough tracks to generate playlists (minimum 5)" }),
            ),
        ));
    }

    // Fetch artist names
    let artist_ids: Vec<Uuid> = all_tracks
        .iter()
        .map(|t| t.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let artists: std::collections::HashMap<Uuid, String> = artist::Entity::find()
        .filter(artist::Column::Id.is_in(artist_ids))
        .all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|a| (a.id, a.name))
        .collect();

    // Fetch album info for covers
    let album_ids: Vec<Uuid> = all_tracks
        .iter()
        .filter_map(|t| t.album_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let albums: std::collections::HashMap<Uuid, album::Model> = if !album_ids.is_empty() {
        album::Entity::find()
            .filter(album::Column::Id.is_in(album_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        std::collections::HashMap::new()
    };

    // Build track list for AI prompt
    #[derive(Serialize)]
    struct TrackInfo {
        id: String,
        title: String,
        artist: String,
        genre: String,
        year: Option<i16>,
    }

    let track_list: Vec<TrackInfo> = all_tracks
        .iter()
        .map(|t| TrackInfo {
            id: t.id.to_string(),
            title: t.title.clone(),
            artist: artists.get(&t.artist_id).cloned().unwrap_or_default(),
            genre: t.genre.clone().unwrap_or_else(|| "Unknown".to_string()),
            year: t.year,
        })
        .collect();

    let track_json = serde_json::to_string(&track_list).unwrap_or_default();

    // 2. Call AI API
    let num_playlists = if all_tracks.len() < 15 {
        2
    } else if all_tracks.len() < 50 {
        4
    } else {
        6
    };
    let tracks_per_playlist = (all_tracks.len() / num_playlists).clamp(5, 25);

    let system_prompt = format!(
        r#"You are a music curator for a streaming platform. Create exactly {num_playlists} themed playlists from the available tracks.

Rules:
- Each playlist should have {tracks_per_playlist} to {} tracks
- Each playlist needs a creative French name (e.g. "Nuit Électrique", "Voyage Acoustique")
- Each playlist needs a short French description (1-2 sentences)
- Group tracks by mood, genre, energy level, or theme
- A track can appear in multiple playlists
- Use ONLY track IDs from the provided list

Respond ONLY with valid JSON in this exact format:
[
  {{
    "name": "Playlist Name",
    "description": "Short description",
    "track_ids": ["uuid1", "uuid2", ...]
  }}
]"#,
        tracks_per_playlist + 5,
    );

    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": format!("Here are the available tracks:\n{track_json}") }
        ],
        "temperature": 0.8,
        "max_tokens": 4000,
    });

    let client = reqwest::Client::new();
    let ai_response = client
        .post(format!("{base_url}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("AI API request failed (details redacted)");
            tracing::debug!("AI API request error kind: {:?}", e.without_url());
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": "AI API request failed" })),
            )
        })?;

    if !ai_response.status().is_success() {
        let status = ai_response.status();
        let body = ai_response.text().await.unwrap_or_default();
        let truncated = if body.len() > 200 {
            &body[..200]
        } else {
            &body
        };
        tracing::error!(%status, "AI API returned error");
        tracing::debug!(body = truncated, "AI API error response (truncated)");
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("AI API returned status {status}") })),
        ));
    }

    let ai_body: serde_json::Value = ai_response.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("Failed to parse AI response: {e}") })),
        )
    })?;

    let content = ai_body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("[]");

    // Strip markdown code fences if present
    let clean_content = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    #[derive(Deserialize)]
    struct AiPlaylist {
        name: String,
        description: String,
        track_ids: Vec<String>,
    }

    let ai_playlists: Vec<AiPlaylist> = serde_json::from_str(clean_content).map_err(|e| {
        let preview = if clean_content.len() > 200 {
            &clean_content[..200]
        } else {
            clean_content
        };
        tracing::error!("Failed to parse AI playlists JSON: {e}");
        tracing::debug!(content_preview = preview, "AI response content (truncated)");
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("AI returned invalid JSON: {e}") })),
        )
    })?;

    // 3. Delete old editorial playlists
    let old_editorials = playlist::Entity::find()
        .filter(playlist::Column::IsEditorial.eq(true))
        .all(&state.db)
        .await
        .unwrap_or_default();

    for old in &old_editorials {
        // Delete tracks first (cascade)
        playlist_track::Entity::delete_many()
            .filter(playlist_track::Column::PlaylistId.eq(old.id))
            .exec(&state.db)
            .await
            .ok();
        playlist::Entity::delete_by_id(old.id)
            .exec(&state.db)
            .await
            .ok();
    }

    // 4. Need a user_id for playlists — use the first admin
    let admin_user = soundtime_db::entities::user::Entity::find()
        .filter(
            soundtime_db::entities::user::Column::Role
                .eq(soundtime_db::entities::user::UserRole::Admin),
        )
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
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "No admin user found" })),
            )
        })?;

    // Valid track IDs set for validation
    let valid_ids: std::collections::HashSet<String> =
        all_tracks.iter().map(|t| t.id.to_string()).collect();

    let now = chrono::Utc::now().fixed_offset();
    let mut created_count = 0;

    for ai_pl in &ai_playlists {
        // Filter to only valid track IDs
        let valid_track_ids: Vec<Uuid> = ai_pl
            .track_ids
            .iter()
            .filter(|id| valid_ids.contains(id.as_str()))
            .filter_map(|id| Uuid::parse_str(id).ok())
            .collect();

        if valid_track_ids.is_empty() {
            continue;
        }

        // Determine cover: use first track's album cover
        let cover_url = valid_track_ids
            .first()
            .and_then(|tid| all_tracks.iter().find(|t| t.id == *tid))
            .and_then(|t| t.album_id)
            .and_then(|aid| albums.get(&aid))
            .and_then(|a| a.cover_url.clone())
            .map(|url| {
                if url.starts_with("/api/media/") || url.starts_with("http") {
                    url
                } else {
                    format!("/api/media/{url}")
                }
            });

        let playlist_id = Uuid::new_v4();
        let new_playlist = playlist::ActiveModel {
            id: Set(playlist_id),
            name: Set(ai_pl.name.clone()),
            description: Set(Some(ai_pl.description.clone())),
            user_id: Set(admin_user.id),
            is_public: Set(true),
            is_editorial: Set(true),
            cover_url: Set(cover_url),
            federation_uri: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };

        if new_playlist.insert(&state.db).await.is_err() {
            continue;
        }

        // Insert tracks
        for (pos, tid) in valid_track_ids.iter().enumerate() {
            let entry = playlist_track::ActiveModel {
                playlist_id: Set(playlist_id),
                track_id: Set(*tid),
                position: Set(pos as i32),
            };
            entry.insert(&state.db).await.ok();
        }

        created_count += 1;
    }

    // Update last generated timestamp
    set_setting(
        &state,
        "editorial_last_generated",
        &chrono::Utc::now().to_rfc3339(),
    )
    .await;

    tracing::info!("Generated {created_count} editorial playlists via AI");

    Ok(Json(GenerateResult {
        playlists_created: created_count,
        message: format!("{created_count} playlists éditoriales créées avec succès"),
    }))
}

/// GET /api/admin/editorial/status — check AI config and last generation time
#[derive(Serialize)]
pub struct EditorialStatus {
    pub ai_configured: bool,
    pub ai_base_url: String,
    pub ai_model: String,
    pub last_generated: Option<String>,
    pub playlist_count: u64,
    pub needs_regeneration: bool,
}

pub async fn editorial_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<EditorialStatus>, (StatusCode, String)> {
    let api_key = get_setting(&state, "ai_api_key").await;
    let ai_configured = api_key.map(|k| !k.is_empty()).unwrap_or(false);
    let base_url = get_setting(&state, "ai_base_url")
        .await
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let model = get_setting(&state, "ai_model")
        .await
        .unwrap_or_else(|| "gpt-4o-mini".to_string());
    let last_generated = get_setting(&state, "editorial_last_generated").await;

    let playlist_count = playlist::Entity::find()
        .filter(playlist::Column::IsEditorial.eq(true))
        .count(&state.db)
        .await
        .unwrap_or(0);

    // Check if regeneration is needed (last generated > 7 days ago or never)
    let needs_regeneration = match &last_generated {
        None => ai_configured,
        Some(ts) => {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                let days_since = (chrono::Utc::now() - dt.with_timezone(&chrono::Utc)).num_days();
                days_since >= 7
            } else {
                true
            }
        }
    };

    Ok(Json(EditorialStatus {
        ai_configured,
        ai_base_url: base_url,
        ai_model: model,
        last_generated,
        playlist_count,
        needs_regeneration,
    }))
}

// ─── Helpers ────────────────────────────────────────────────────────

async fn get_setting(state: &AppState, key: &str) -> Option<String> {
    instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq(key))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value)
}

async fn set_setting(state: &AppState, key: &str, value: &str) {
    let now = chrono::Utc::now().fixed_offset();
    if let Ok(Some(existing)) = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq(key))
        .one(&state.db)
        .await
    {
        let mut active: instance_setting::ActiveModel = existing.into();
        active.value = Set(value.to_string());
        active.updated_at = Set(now);
        active.update(&state.db).await.ok();
    } else {
        let new_setting = instance_setting::ActiveModel {
            id: Set(Uuid::new_v4()),
            key: Set(key.to_string()),
            value: Set(value.to_string()),
            updated_at: Set(now),
        };
        new_setting.insert(&state.db).await.ok();
    }
}

// ─── Background auto-regeneration ───────────────────────────────────

/// Called once at startup — spawns a background task that checks weekly
pub fn spawn_editorial_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        // Wait 30 seconds after startup before first check
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;

        loop {
            // Check if AI is configured
            let api_key = get_setting(&state, "ai_api_key").await;
            if let Some(key) = api_key {
                if !key.is_empty() {
                    // Check if it's Saturday and needs regeneration
                    let now = chrono::Utc::now();
                    let is_saturday = now.format("%A").to_string() == "Saturday";
                    let last_gen = get_setting(&state, "editorial_last_generated").await;

                    let should_generate = match &last_gen {
                        None => true,
                        Some(ts) => {
                            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                                let days_since = (now - dt.with_timezone(&chrono::Utc)).num_days();
                                is_saturday && days_since >= 6
                            } else {
                                true
                            }
                        }
                    };

                    if should_generate {
                        tracing::info!("Auto-generating editorial playlists (Saturday schedule)");
                        // Reuse same logic — create a mock State extractor
                        let state_clone = state.clone();
                        match generate_editorial_inner(&state_clone).await {
                            Ok(count) => {
                                tracing::info!("Auto-generated {count} editorial playlists");
                            }
                            Err(e) => {
                                tracing::warn!("Editorial auto-generation failed: {e}");
                            }
                        }
                    }
                }
            }

            // Check every 6 hours
            tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
        }
    });
}

/// Inner generation function (reusable from scheduler and endpoint)
async fn generate_editorial_inner(state: &AppState) -> Result<usize, String> {
    let api_key = get_setting(state, "ai_api_key")
        .await
        .ok_or("AI API key not configured")?;
    if api_key.is_empty() {
        return Err("AI API key is empty".to_string());
    }

    let base_url = get_setting(state, "ai_base_url")
        .await
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let model = get_setting(state, "ai_model")
        .await
        .unwrap_or_else(|| "gpt-4o-mini".to_string());

    let all_tracks = track::Entity::find()
        .order_by_asc(track::Column::Title)
        .all(&state.db)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    if all_tracks.len() < 5 {
        return Err("Not enough tracks".to_string());
    }

    let artist_ids: Vec<Uuid> = all_tracks
        .iter()
        .map(|t| t.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let artists_map: std::collections::HashMap<Uuid, String> = artist::Entity::find()
        .filter(artist::Column::Id.is_in(artist_ids))
        .all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|a| (a.id, a.name))
        .collect();

    let album_ids: Vec<Uuid> = all_tracks
        .iter()
        .filter_map(|t| t.album_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let albums_map: std::collections::HashMap<Uuid, album::Model> = if !album_ids.is_empty() {
        album::Entity::find()
            .filter(album::Column::Id.is_in(album_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        std::collections::HashMap::new()
    };

    #[derive(Serialize)]
    struct TrackInfo {
        id: String,
        title: String,
        artist: String,
        genre: String,
        year: Option<i16>,
    }

    let track_list: Vec<TrackInfo> = all_tracks
        .iter()
        .map(|t| TrackInfo {
            id: t.id.to_string(),
            title: t.title.clone(),
            artist: artists_map.get(&t.artist_id).cloned().unwrap_or_default(),
            genre: t.genre.clone().unwrap_or_else(|| "Unknown".to_string()),
            year: t.year,
        })
        .collect();

    let track_json = serde_json::to_string(&track_list).unwrap_or_default();
    let num_playlists = if all_tracks.len() < 15 {
        2
    } else if all_tracks.len() < 50 {
        4
    } else {
        6
    };
    let tracks_per_playlist = (all_tracks.len() / num_playlists).clamp(5, 25);

    let system_prompt = format!(
        r#"You are a music curator for a streaming platform. Create exactly {num_playlists} themed playlists from the available tracks.

Rules:
- Each playlist should have {tracks_per_playlist} to {} tracks
- Each playlist needs a creative name
- Each playlist needs a short description (1-2 sentences)
- Group tracks by mood, genre, energy level, or theme
- A track can appear in multiple playlists
- Use ONLY track IDs from the provided list

Respond ONLY with valid JSON in this exact format:
[
  {{
    "name": "Playlist Name",
    "description": "Short description",
    "track_ids": ["uuid1", "uuid2", ...]
  }}
]"#,
        tracks_per_playlist + 5,
    );

    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": format!("Here are the available tracks:\n{track_json}") }
        ],
        "temperature": 0.8,
        "max_tokens": 4000,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/chat/completions"))
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            tracing::debug!("AI API request error kind: {:?}", e.without_url());
            "AI API request failed".to_string()
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let _body = resp.text().await.unwrap_or_default();
        return Err(format!("AI API returned status {status}"));
    }

    let ai_body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse AI response: {e}"))?;

    let content = ai_body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("[]");

    let clean_content = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    #[derive(Deserialize)]
    struct AiPlaylist {
        name: String,
        description: String,
        track_ids: Vec<String>,
    }

    let ai_playlists: Vec<AiPlaylist> = serde_json::from_str(clean_content)
        .map_err(|e| format!("AI returned invalid JSON: {e}"))?;

    // Delete old editorial playlists
    let old_editorials = playlist::Entity::find()
        .filter(playlist::Column::IsEditorial.eq(true))
        .all(&state.db)
        .await
        .unwrap_or_default();

    for old in &old_editorials {
        playlist_track::Entity::delete_many()
            .filter(playlist_track::Column::PlaylistId.eq(old.id))
            .exec(&state.db)
            .await
            .ok();
        playlist::Entity::delete_by_id(old.id)
            .exec(&state.db)
            .await
            .ok();
    }

    let admin_user = soundtime_db::entities::user::Entity::find()
        .filter(
            soundtime_db::entities::user::Column::Role
                .eq(soundtime_db::entities::user::UserRole::Admin),
        )
        .one(&state.db)
        .await
        .map_err(|e| format!("DB error: {e}"))?
        .ok_or("No admin user found")?;

    let valid_ids: std::collections::HashSet<String> =
        all_tracks.iter().map(|t| t.id.to_string()).collect();

    let now = chrono::Utc::now().fixed_offset();
    let mut created_count = 0;

    for ai_pl in &ai_playlists {
        let valid_track_ids: Vec<Uuid> = ai_pl
            .track_ids
            .iter()
            .filter(|id| valid_ids.contains(id.as_str()))
            .filter_map(|id| Uuid::parse_str(id).ok())
            .collect();

        if valid_track_ids.is_empty() {
            continue;
        }

        let cover_url = valid_track_ids
            .first()
            .and_then(|tid| all_tracks.iter().find(|t| t.id == *tid))
            .and_then(|t| t.album_id)
            .and_then(|aid| albums_map.get(&aid))
            .and_then(|a| a.cover_url.clone())
            .map(|url| {
                if url.starts_with("/api/media/") || url.starts_with("http") {
                    url
                } else {
                    format!("/api/media/{url}")
                }
            });

        let playlist_id = Uuid::new_v4();
        let new_playlist = playlist::ActiveModel {
            id: Set(playlist_id),
            name: Set(ai_pl.name.clone()),
            description: Set(Some(ai_pl.description.clone())),
            user_id: Set(admin_user.id),
            is_public: Set(true),
            is_editorial: Set(true),
            cover_url: Set(cover_url),
            federation_uri: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };

        if new_playlist.insert(&state.db).await.is_err() {
            continue;
        }

        for (pos, tid) in valid_track_ids.iter().enumerate() {
            let entry = playlist_track::ActiveModel {
                playlist_id: Set(playlist_id),
                track_id: Set(*tid),
                position: Set(pos as i32),
            };
            entry.insert(&state.db).await.ok();
        }

        created_count += 1;
    }

    set_setting(
        state,
        "editorial_last_generated",
        &chrono::Utc::now().to_rfc3339(),
    )
    .await;
    Ok(created_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_generate_result() {
        let result = GenerateResult {
            playlists_created: 4,
            message: "4 playlists créées".to_string(),
        };
        let val = serde_json::to_value(&result).unwrap();
        assert_eq!(val["playlists_created"], 4);
        assert_eq!(val["message"], "4 playlists créées");
    }

    #[test]
    fn test_serialize_editorial_status() {
        let status = EditorialStatus {
            ai_configured: true,
            ai_base_url: "https://api.openai.com/v1".to_string(),
            ai_model: "gpt-4o-mini".to_string(),
            last_generated: Some("2024-06-01T00:00:00Z".to_string()),
            playlist_count: 4,
            needs_regeneration: false,
        };
        let val = serde_json::to_value(&status).unwrap();
        assert_eq!(val["ai_configured"], true);
        assert_eq!(val["ai_model"], "gpt-4o-mini");
        assert_eq!(val["playlist_count"], 4);
    }

    #[test]
    fn test_serialize_editorial_status_not_configured() {
        let status = EditorialStatus {
            ai_configured: false,
            ai_base_url: "https://api.openai.com/v1".to_string(),
            ai_model: "gpt-4o-mini".to_string(),
            last_generated: None,
            playlist_count: 0,
            needs_regeneration: false,
        };
        let val = serde_json::to_value(&status).unwrap();
        assert_eq!(val["ai_configured"], false);
        assert!(val["last_generated"].is_null());
    }
}
