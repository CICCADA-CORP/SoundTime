use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
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

    Ok(StatusCode::CREATED)
}
