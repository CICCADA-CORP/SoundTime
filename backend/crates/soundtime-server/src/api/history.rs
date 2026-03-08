//! Listen history — recording and querying what users have played.
//!
//! Provides endpoints for logging listens (`POST /api/history`) and retrieving
//! paginated or recent listen history (`GET /api/history`, `GET /api/history/recent`).
//! Track data is batch-fetched to avoid N+1 query patterns.

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

/// A single entry in the user's listen history, combining the listen
/// metadata (timestamp, duration) with the full track response.
#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub track: super::tracks::TrackResponse,
    pub listened_at: chrono::DateTime<chrono::FixedOffset>,
    pub duration_listened: f32,
}

/// Request body for `POST /api/history` — records a listen event with
/// optional behavioral signals (Phase 2).
///
/// The four behavioral fields (`source_context`, `completed`, `skipped`,
/// `skip_position`) are all `Option` / `#[serde(default)]` so that older
/// clients that don't send them continue to work without changes.
#[derive(Debug, Deserialize)]
pub struct LogListenRequest {
    /// UUID of the track that was played.
    pub track_id: Uuid,
    /// How many seconds the user actually listened (may be less than track duration).
    pub duration_listened: f32,
    /// Where the track was played from (e.g. "album", "playlist", "radio",
    /// "search", "queue", "collection", "explore"). Used by the recommendation
    /// engine to weigh intentional plays higher than auto-queued ones.
    #[serde(default)]
    pub source_context: Option<String>,
    /// `true` when the track finished naturally (the `ended` event fired),
    /// `false` when the user switched away before the track ended.
    #[serde(default)]
    pub completed: Option<bool>,
    /// `true` when the user actively skipped to another track before this one
    /// finished. A skip is a negative signal for recommendations.
    #[serde(default)]
    pub skipped: Option<bool>,
    /// Playback position (seconds) at the moment the user skipped. Only
    /// meaningful when `skipped` is `true`; helps distinguish early skips
    /// (strong negative signal) from late skips (near-completion).
    #[serde(default)]
    pub skip_position: Option<f32>,
}

/// GET /api/history (auth required)
///
/// Returns a paginated list of the authenticated user's listen history,
/// ordered by most recent first. Track data is batch-fetched using an
/// `IN` clause rather than individual queries per entry, avoiding the
/// N+1 query problem and keeping database round-trips constant
/// regardless of page size.
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

    // PERF: Batch-fetch all referenced tracks in a single query using `IS IN`,
    // instead of querying each track individually (N+1 → 1 query).
    let track_ids: Vec<Uuid> = entries.iter().map(|e| e.track_id).collect();
    let tracks_map: HashMap<Uuid, track::Model> = if !track_ids.is_empty() {
        track::Entity::find()
            .filter(track::Column::Id.is_in(track_ids))
            .all(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
            .into_iter()
            .map(|t| (t.id, t))
            .collect()
    } else {
        HashMap::new()
    };

    let data: Vec<HistoryEntry> = entries
        .into_iter()
        .filter_map(|entry| {
            let t = tracks_map.get(&entry.track_id)?;
            Some(HistoryEntry {
                id: entry.id,
                track: super::tracks::TrackResponse::from(t.clone()),
                listened_at: entry.listened_at,
                duration_listened: entry.duration_listened,
            })
        })
        .collect();

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
///
/// Records a new listen history entry and atomically increments the track's
/// `play_count` using a SQL `SET play_count = play_count + 1` statement.
/// The atomic update prevents race conditions that would occur under
/// concurrent requests if we did a separate read-modify-write cycle.
///
/// Phase 2 behavioral signals (`source_context`, `completed`, `skipped`,
/// `skip_position`) are persisted alongside the core listen data. These
/// optional fields are forwarded directly into the `listen_history` row and
/// are consumed downstream by the recommendation engine.
///
/// Also dispatches plugin events and Last.fm scrobbles on a best-effort,
/// fire-and-forget basis (failures are logged but do not fail the request).
///
/// On every 10th listen, recomputes the user's taste vector (a weighted
/// average of track embeddings) used by the similarity radio seed.
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
        source_context: Set(body.source_context.clone()),
        completed: Set(body.completed),
        skipped: Set(body.skipped),
        skip_position: Set(body.skip_position),
    };

    entry
        .insert(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    // SECURITY: Atomic increment avoids TOCTOU race where two concurrent
    // requests could read the same play_count and both write count+1,
    // losing an increment. The database handles serialisation for us.
    {
        use sea_orm::{ConnectionTrait, Statement};
        if let Err(e) = state
            .db
            .execute(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                "UPDATE tracks SET play_count = play_count + 1 WHERE id = $1",
                [body.track_id.into()],
            ))
            .await
        {
            tracing::warn!(error = %e, track_id = %body.track_id, "failed to increment play count");
        }
    }

    // Update Redis trending scores (best-effort, fire-and-forget)
    #[cfg(feature = "redis")]
    {
        if let Some(ref pool) = state.redis {
            let pool = pool.clone();
            let track_id = body.track_id;
            let completed = body.completed.unwrap_or(false);
            let skipped = body.skipped.unwrap_or(false);
            tokio::spawn(async move {
                crate::trending::record_play(&pool, track_id, completed, skipped).await;
            });
        }
    }

    // Update user taste vector (best-effort, throttled — every 10th listen)
    //
    // The taste vector is a weighted average of the user's listened track
    // embeddings, stored in `user_taste_vectors` for recommendation queries.
    // Updating it on every listen would be wasteful, so we only recompute
    // every 10 listens. The vector naturally evolves as listening patterns
    // change over time.
    {
        let db = state.db.clone();
        let user_id = auth_user.0.sub;
        tokio::spawn(async move {
            let listen_count = listen_history::Entity::find()
                .filter(listen_history::Column::UserId.eq(user_id))
                .count(&db)
                .await
                .unwrap_or(0);

            if listen_count % 10 == 0 {
                if let Err(e) = crate::embeddings::update_user_taste_vector(&db, user_id).await {
                    tracing::warn!(error = %e, user_id = %user_id, "failed to update user taste vector");
                }
            }
        });
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

    // Scrobble to Last.fm (best-effort, fire-and-forget)
    {
        let db = state.db.clone();
        let user_id = auth_user.0.sub;
        let track_id = body.track_id;
        let duration_listened = body.duration_listened;
        let timestamp = chrono::Utc::now().fixed_offset();
        let jwt_secret = state.jwt_secret.clone();
        tokio::spawn(async move {
            if let Err(e) = super::lastfm::scrobble_for_user(
                &db,
                user_id,
                track_id,
                duration_listened,
                timestamp,
                &jwt_secret,
            )
            .await
            {
                tracing::warn!(error = %e, "Last.fm scrobble failed");
            }
        });
    }

    Ok(StatusCode::CREATED)
}

/// GET /api/history/recent — recent listens with batch-fetched track data (auth required)
///
/// Returns up to `per_page` (default 6, max 50) recently listened tracks,
/// deduplicated by track ID (keeping only the most recent listen per track).
/// Over-fetches `limit * 10` rows to have enough candidates after dedup.
///
/// All related data (tracks, artists, albums) is batch-fetched using `IS IN`
/// queries to avoid N+1 patterns — a total of 4 queries regardless of result size.
pub async fn list_recent_history(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<Vec<HistoryEntry>>, (StatusCode, String)> {
    let limit = params.per_page.unwrap_or(6).min(50);

    // Fetch more rows than needed so we can deduplicate by track_id in Rust,
    // keeping only the most recent listen per track.
    let raw_entries = listen_history::Entity::find()
        .filter(listen_history::Column::UserId.eq(auth_user.0.sub))
        .order_by_desc(listen_history::Column::ListenedAt)
        .limit(limit * 10)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    // Deduplicate: keep only the first (most recent) listen per track_id
    let mut seen_tracks: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    let entries: Vec<listen_history::Model> = raw_entries
        .into_iter()
        .filter(|e| seen_tracks.insert(e.track_id))
        .take(limit as usize)
        .collect();

    if entries.is_empty() {
        return Ok(Json(vec![]));
    }

    // PERF: Batch-fetch all tracks in a single query using `IS IN`.
    let track_ids: Vec<Uuid> = entries.iter().map(|e| e.track_id).collect();
    let tracks_map: HashMap<Uuid, track::Model> = track::Entity::find()
        .filter(track::Column::Id.is_in(track_ids))
        .all(&state.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|t| (t.id, t))
        .collect();

    // PERF: Batch-fetch artists and albums for those tracks (2 additional queries).
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

    #[test]
    fn test_deserialize_log_listen_request_with_behavioral_fields() {
        let json = r#"{
            "track_id": "550e8400-e29b-41d4-a716-446655440000",
            "duration_listened": 45.2,
            "source_context": "radio",
            "completed": false,
            "skipped": true,
            "skip_position": 45.2
        }"#;
        let req: LogListenRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.source_context, Some("radio".to_string()));
        assert_eq!(req.completed, Some(false));
        assert_eq!(req.skipped, Some(true));
        assert!((req.skip_position.unwrap() - 45.2).abs() < f32::EPSILON);
    }

    #[test]
    fn test_deserialize_log_listen_request_backward_compatible() {
        let json =
            r#"{"track_id":"550e8400-e29b-41d4-a716-446655440000","duration_listened":120.5}"#;
        let req: LogListenRequest = serde_json::from_str(json).unwrap();
        assert!(req.source_context.is_none());
        assert!(req.completed.is_none());
        assert!(req.skipped.is_none());
        assert!(req.skip_position.is_none());
    }
}
