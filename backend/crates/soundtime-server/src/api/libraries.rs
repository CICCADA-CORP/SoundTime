use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use super::tracks::PaginationParams;
use soundtime_db::entities::{library, library_track, track};
use soundtime_db::AppState;

#[derive(Debug, Serialize)]
pub struct LibraryResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub user_id: Uuid,
    pub is_public: bool,
    pub total_tracks: i32,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
}

impl From<library::Model> for LibraryResponse {
    fn from(l: library::Model) -> Self {
        Self {
            id: l.id,
            name: l.name,
            description: l.description,
            user_id: l.user_id,
            is_public: l.is_public,
            total_tracks: l.total_tracks,
            created_at: l.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LibraryDetailResponse {
    #[serde(flatten)]
    pub library: LibraryResponse,
    pub tracks: Vec<super::tracks::TrackResponse>,
}

/// GET /api/libraries
pub async fn list_libraries(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<super::tracks::PaginatedResponse<LibraryResponse>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);

    let paginator = library::Entity::find()
        .filter(library::Column::IsPublic.eq(true))
        .order_by_desc(library::Column::CreatedAt)
        .paginate(&state.db, per_page);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let libs = paginator
        .fetch_page(page - 1)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_pages = (total + per_page - 1) / per_page;

    Ok(Json(super::tracks::PaginatedResponse {
        data: libs.into_iter().map(LibraryResponse::from).collect(),
        total,
        page,
        per_page,
        total_pages,
    }))
}

/// GET /api/libraries/:id
pub async fn get_library(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<LibraryDetailResponse>, (StatusCode, String)> {
    let lib = library::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Library not found".to_string()))?;

    let lt_entries = library_track::Entity::find()
        .filter(library_track::Column::LibraryId.eq(id))
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let track_ids: Vec<Uuid> = lt_entries.iter().map(|lt| lt.track_id).collect();

    let tracks = if track_ids.is_empty() {
        vec![]
    } else {
        track::Entity::find()
            .filter(track::Column::Id.is_in(track_ids))
            .all(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
            .into_iter()
            .map(super::tracks::TrackResponse::from)
            .collect()
    };

    Ok(Json(LibraryDetailResponse {
        library: LibraryResponse::from(lib),
        tracks,
    }))
}
