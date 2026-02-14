use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use super::tracks::PaginationParams;
use crate::auth::middleware::AuthUser;
use soundtime_db::entities::{listen_history, track};
use soundtime_db::AppState;

#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub track: super::tracks::TrackResponse,
    pub listened_at: chrono::DateTime<chrono::FixedOffset>,
    pub duration_listened: f32,
}

#[derive(Debug, Deserialize)]
pub struct LogListenRequest {
    pub track_id: Uuid,
    pub duration_listened: f32,
}

/// GET /api/history (auth required)
pub async fn list_history(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<super::tracks::PaginatedResponse<HistoryEntry>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);

    let paginator = listen_history::Entity::find()
        .filter(listen_history::Column::UserId.eq(auth_user.0.sub))
        .order_by_desc(listen_history::Column::ListenedAt)
        .paginate(&state.db, per_page);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let entries = paginator
        .fetch_page(page - 1)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let mut data = Vec::new();
    for entry in entries {
        if let Some(t) = track::Entity::find_by_id(entry.track_id)
            .one(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        {
            data.push(HistoryEntry {
                id: entry.id,
                track: super::tracks::TrackResponse::from(t),
                listened_at: entry.listened_at,
                duration_listened: entry.duration_listened,
            });
        }
    }

    let total_pages = total.div_ceil(per_page);

    Ok(Json(super::tracks::PaginatedResponse {
        data,
        total,
        page,
        per_page,
        total_pages,
    }))
}

/// POST /api/history (auth required — log a listen)
pub async fn log_listen(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Json(body): Json<LogListenRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let entry = listen_history::ActiveModel {
        id: Set(Uuid::new_v4()),
        user_id: Set(auth_user.0.sub),
        track_id: Set(body.track_id),
        listened_at: Set(chrono::Utc::now().fixed_offset()),
        duration_listened: Set(body.duration_listened),
    };

    entry
        .insert(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    // Increment play_count on the track
    let track_model = track::Entity::find_by_id(body.track_id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
    if let Some(t) = track_model {
        let mut update: track::ActiveModel = t.into();
        update.play_count = Set(update.play_count.unwrap() + 1);
        let _ = update.update(&state.db).await;
    }

    // Dispatch plugin event (best-effort)
    if let Some(registry) = super::get_plugin_registry(&state) {
        let payload = soundtime_plugin::TrackPlayedPayload {
            track_id: body.track_id.to_string(),
            user_id: auth_user.0.sub.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        let payload_val = serde_json::to_value(&payload).unwrap_or_default();
        let registry = registry.clone();
        tokio::spawn(async move {
            registry.dispatch("on_track_played", &payload_val).await;
        });
    }

    Ok(StatusCode::CREATED)
}

/// GET /api/history/recent — recent listens with batch-fetched track data (auth required)
pub async fn list_recent_history(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Vec<HistoryEntry>>, (StatusCode, String)> {
    let limit = params.per_page.unwrap_or(6).min(50);

    let entries = listen_history::Entity::find()
        .filter(listen_history::Column::UserId.eq(auth_user.0.sub))
        .order_by_desc(listen_history::Column::ListenedAt)
        .limit(limit)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    if entries.is_empty() {
        return Ok(Json(vec![]));
    }

    // Batch-fetch all tracks
    let track_ids: Vec<Uuid> = entries
        .iter()
        .map(|e| e.track_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let tracks_map: HashMap<Uuid, track::Model> = track::Entity::find()
        .filter(track::Column::Id.is_in(track_ids))
        .all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|t| (t.id, t))
        .collect();

    // Batch-fetch artists and albums for those tracks
    let artist_ids: Vec<Uuid> = tracks_map
        .values()
        .map(|t| t.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let album_ids: Vec<Uuid> = tracks_map
        .values()
        .filter_map(|t| t.album_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let artists: HashMap<Uuid, soundtime_db::entities::artist::Model> = if !artist_ids.is_empty() {
        soundtime_db::entities::artist::Entity::find()
            .filter(soundtime_db::entities::artist::Column::Id.is_in(artist_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let albums: HashMap<Uuid, soundtime_db::entities::album::Model> = if !album_ids.is_empty() {
        soundtime_db::entities::album::Entity::find()
            .filter(soundtime_db::entities::album::Column::Id.is_in(album_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let data = entries
        .into_iter()
        .filter_map(|entry| {
            let t = tracks_map.get(&entry.track_id)?;
            let artist_name = artists.get(&t.artist_id).map(|a| a.name.clone());
            let (album_title, cover_url) = t
                .album_id
                .and_then(|aid| albums.get(&aid))
                .map(|a| {
                    (
                        Some(a.title.clone()),
                        a.cover_url.clone().map(|url| {
                            if url.starts_with("/api/media/") || url.starts_with("http") {
                                url
                            } else {
                                format!("/api/media/{url}")
                            }
                        }),
                    )
                })
                .unwrap_or((None, None));

            let mut resp = super::tracks::TrackResponse::from(t.clone());
            resp.artist_name = artist_name;
            resp.album_title = album_title;
            resp.cover_url = cover_url;

            Some(HistoryEntry {
                id: entry.id,
                track: resp,
                listened_at: entry.listened_at,
                duration_listened: entry.duration_listened,
            })
        })
        .collect();

    Ok(Json(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_log_listen_request() {
        let json =
            r#"{"track_id":"550e8400-e29b-41d4-a716-446655440000","duration_listened":120.5}"#;
        let req: LogListenRequest = serde_json::from_str(json).unwrap();
        assert_eq!(
            req.track_id.to_string(),
            "550e8400-e29b-41d4-a716-446655440000"
        );
        assert!((req.duration_listened - 120.5).abs() < f32::EPSILON);
    }
}
