use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use super::tracks::PaginationParams;
use crate::auth::middleware::AuthUser;
use soundtime_db::entities::{favorite, track};
use soundtime_db::AppState;

/// GET /api/favorites (auth required)
pub async fn list_favorites(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<super::tracks::PaginatedResponse<super::tracks::TrackResponse>>, (StatusCode, String)>
{
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);

    let paginator = favorite::Entity::find()
        .filter(favorite::Column::UserId.eq(auth_user.0.sub))
        .order_by_desc(favorite::Column::CreatedAt)
        .paginate(&state.db, per_page);

    let total = paginator.num_items().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    let favs = paginator.fetch_page(page - 1).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    let mut data = Vec::new();
    for fav in favs {
        if let Some(t) = track::Entity::find_by_id(fav.track_id)
            .one(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        {
            data.push(super::tracks::TrackResponse::from(t));
        }
    }

    let total_pages = (total + per_page - 1) / per_page;

    Ok(Json(super::tracks::PaginatedResponse {
        data,
        total,
        page,
        per_page,
        total_pages,
    }))
}

/// POST /api/favorites/:track_id (auth required)
pub async fn add_favorite(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Path(track_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Check if already favorited
    let existing = favorite::Entity::find()
        .filter(favorite::Column::UserId.eq(auth_user.0.sub))
        .filter(favorite::Column::TrackId.eq(track_id))
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    if existing.is_some() {
        return Ok(StatusCode::NO_CONTENT);
    }

    let entry = favorite::ActiveModel {
        user_id: Set(auth_user.0.sub),
        track_id: Set(track_id),
        created_at: Set(chrono::Utc::now().fixed_offset()),
    };

    entry.insert(&state.db).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/favorites/check?track_ids=id1,id2,... (auth required)
pub async fn check_favorites(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Query(params): Query<CheckFavoritesParams>,
) -> Result<Json<Vec<String>>, (StatusCode, String)> {
    let ids: Vec<Uuid> = params
        .track_ids
        .split(',')
        .filter_map(|s| s.trim().parse::<Uuid>().ok())
        .collect();

    if ids.is_empty() {
        return Ok(Json(vec![]));
    }

    let favs = favorite::Entity::find()
        .filter(favorite::Column::UserId.eq(auth_user.0.sub))
        .filter(favorite::Column::TrackId.is_in(ids))
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(favs.into_iter().map(|f| f.track_id.to_string()).collect()))
}

#[derive(Deserialize)]
pub struct CheckFavoritesParams {
    pub track_ids: String,
}

/// DELETE /api/favorites/:track_id (auth required)
pub async fn remove_favorite(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Path(track_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    favorite::Entity::delete_many()
        .filter(favorite::Column::UserId.eq(auth_user.0.sub))
        .filter(favorite::Column::TrackId.eq(track_id))
        .exec(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(StatusCode::NO_CONTENT)
}
