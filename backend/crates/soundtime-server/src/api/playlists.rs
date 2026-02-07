use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use super::tracks::PaginationParams;
use crate::auth::middleware::AuthUser;
use soundtime_db::entities::{playlist, playlist_track, track};
use soundtime_db::AppState;

#[derive(Debug, Serialize)]
pub struct PlaylistResponse {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub user_id: Uuid,
    pub is_public: bool,
    pub is_editorial: bool,
    pub cover_url: Option<String>,
    pub track_count: Option<u64>,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
    pub updated_at: chrono::DateTime<chrono::FixedOffset>,
}

impl From<playlist::Model> for PlaylistResponse {
    fn from(p: playlist::Model) -> Self {
        Self {
            id: p.id,
            name: p.name,
            description: p.description,
            user_id: p.user_id,
            is_public: p.is_public,
            is_editorial: p.is_editorial,
            cover_url: p.cover_url,
            track_count: None,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PlaylistDetailResponse {
    #[serde(flatten)]
    pub playlist: PlaylistResponse,
    pub tracks: Vec<super::tracks::TrackResponse>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePlaylistRequest {
    pub name: String,
    pub description: Option<String>,
    pub is_public: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePlaylistRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_public: Option<bool>,
    pub cover_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddTrackRequest {
    pub track_id: Uuid,
    pub position: Option<i32>,
}

/// GET /api/playlists (public playlists)
pub async fn list_playlists(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<super::tracks::PaginatedResponse<PlaylistResponse>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);

    let paginator = playlist::Entity::find()
        .filter(playlist::Column::IsPublic.eq(true))
        .order_by_desc(playlist::Column::UpdatedAt)
        .paginate(&state.db, per_page);

    let total = paginator.num_items().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    let playlists = paginator.fetch_page(page - 1).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    // Populate track counts
    let mut data: Vec<PlaylistResponse> = Vec::with_capacity(playlists.len());
    for p in playlists {
        let count = playlist_track::Entity::find()
            .filter(playlist_track::Column::PlaylistId.eq(p.id))
            .count(&state.db)
            .await
            .unwrap_or(0);
        let mut resp = PlaylistResponse::from(p);
        resp.track_count = Some(count);
        data.push(resp);
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

/// GET /api/playlists/:id
pub async fn get_playlist(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    auth_user: Option<axum::Extension<AuthUser>>,
) -> Result<Json<PlaylistDetailResponse>, (StatusCode, String)> {
    let playlist_model = playlist::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Playlist not found".to_string()))?;

    // SECURITY: non-public playlists are only visible to their owner
    if !playlist_model.is_public {
        let is_owner = auth_user
            .as_ref()
            .map(|u| u.0 .0.sub == playlist_model.user_id)
            .unwrap_or(false);
        if !is_owner {
            return Err((StatusCode::NOT_FOUND, "Playlist not found".to_string()));
        }
    }

    // Get track IDs from junction table ordered by position
    let pt_entries = playlist_track::Entity::find()
        .filter(playlist_track::Column::PlaylistId.eq(id))
        .order_by_asc(playlist_track::Column::Position)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let track_ids: Vec<Uuid> = pt_entries.iter().map(|pt| pt.track_id).collect();

    let tracks = if track_ids.is_empty() {
        vec![]
    } else {
        track::Entity::find()
            .filter(track::Column::Id.is_in(track_ids.clone()))
            .all(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
    };

    // Reorder tracks by position
    let mut ordered_tracks: Vec<super::tracks::TrackResponse> = Vec::new();
    for tid in &track_ids {
        if let Some(t) = tracks.iter().find(|t| &t.id == tid) {
            ordered_tracks.push(super::tracks::TrackResponse::from(t.clone()));
        }
    }

    Ok(Json(PlaylistDetailResponse {
        playlist: PlaylistResponse::from(playlist_model),
        tracks: ordered_tracks,
    }))
}

/// POST /api/playlists (auth required)
pub async fn create_playlist(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Json(body): Json<CreatePlaylistRequest>,
) -> Result<(StatusCode, Json<PlaylistResponse>), (StatusCode, String)> {
    let now = chrono::Utc::now().fixed_offset();
    let id = Uuid::new_v4();

    let new_playlist = playlist::ActiveModel {
        id: Set(id),
        name: Set(body.name),
        description: Set(body.description),
        user_id: Set(auth_user.0.sub),
        is_public: Set(body.is_public.unwrap_or(false)),
        is_editorial: Set(false),
        cover_url: Set(None),
        federation_uri: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    };

    let created = new_playlist.insert(&state.db).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    Ok((StatusCode::CREATED, Json(PlaylistResponse::from(created))))
}

/// PUT /api/playlists/:id (auth required, owner only)
pub async fn update_playlist(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdatePlaylistRequest>,
) -> Result<Json<PlaylistResponse>, (StatusCode, String)> {
    let existing = playlist::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Playlist not found".to_string()))?;

    if existing.user_id != auth_user.0.sub {
        return Err((StatusCode::FORBIDDEN, "Not your playlist".to_string()));
    }

    let mut active: playlist::ActiveModel = existing.into();
    if let Some(name) = body.name {
        active.name = Set(name);
    }
    if let Some(desc) = body.description {
        active.description = Set(Some(desc));
    }
    if let Some(is_public) = body.is_public {
        active.is_public = Set(is_public);
    }
    if let Some(cover_url) = body.cover_url {
        active.cover_url = Set(Some(cover_url));
    }
    active.updated_at = Set(chrono::Utc::now().fixed_offset());

    let updated = active.update(&state.db).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    Ok(Json(PlaylistResponse::from(updated)))
}

/// DELETE /api/playlists/:id (auth required, owner only)
pub async fn delete_playlist(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let existing = playlist::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Playlist not found".to_string()))?;

    if existing.user_id != auth_user.0.sub {
        return Err((StatusCode::FORBIDDEN, "Not your playlist".to_string()));
    }

    playlist::Entity::delete_by_id(id)
        .exec(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/playlists/:id/tracks (auth required, owner only)
pub async fn add_track_to_playlist(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<AddTrackRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let existing = playlist::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Playlist not found".to_string()))?;

    if existing.user_id != auth_user.0.sub {
        return Err((StatusCode::FORBIDDEN, "Not your playlist".to_string()));
    }

    // Determine position
    let max_pos: i32 = playlist_track::Entity::find()
        .filter(playlist_track::Column::PlaylistId.eq(id))
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .iter()
        .map(|pt| pt.position)
        .max()
        .unwrap_or(-1);

    let position = body.position.unwrap_or(max_pos + 1);

    let new_entry = playlist_track::ActiveModel {
        playlist_id: Set(id),
        track_id: Set(body.track_id),
        position: Set(position),
    };

    new_entry.insert(&state.db).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    Ok(StatusCode::CREATED)
}

/// DELETE /api/playlists/:id/tracks/:track_id (auth required, owner only)
pub async fn remove_track_from_playlist(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Path((id, track_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, (StatusCode, String)> {
    let existing = playlist::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Playlist not found".to_string()))?;

    if existing.user_id != auth_user.0.sub {
        return Err((StatusCode::FORBIDDEN, "Not your playlist".to_string()));
    }

    playlist_track::Entity::delete_many()
        .filter(playlist_track::Column::PlaylistId.eq(id))
        .filter(playlist_track::Column::TrackId.eq(track_id))
        .exec(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_playlist_model() -> playlist::Model {
        playlist::Model {
            id: Uuid::new_v4(),
            name: "My Playlist".into(),
            description: Some("A great playlist".into()),
            user_id: Uuid::new_v4(),
            is_public: true,
            is_editorial: false,
            cover_url: Some("https://img.example.com/playlist.jpg".into()),
            federation_uri: None,
            created_at: Utc::now().fixed_offset(),
            updated_at: Utc::now().fixed_offset(),
        }
    }

    #[test]
    fn test_playlist_response_from_model() {
        let model = make_playlist_model();
        let id = model.id;
        let resp = PlaylistResponse::from(model);
        assert_eq!(resp.id, id);
        assert_eq!(resp.name, "My Playlist");
        assert!(resp.is_public);
        assert!(!resp.is_editorial);
    }

    #[test]
    fn test_playlist_response_serialization() {
        let model = make_playlist_model();
        let resp = PlaylistResponse::from(model);
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["name"], "My Playlist");
        assert_eq!(json["is_public"], true);
        assert_eq!(json["is_editorial"], false);
    }

    #[test]
    fn test_create_playlist_request_deserialization() {
        let json = r#"{"name": "Test", "description": "desc", "is_public": false}"#;
        let req: CreatePlaylistRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "Test");
        assert_eq!(req.description.as_deref(), Some("desc"));
        assert_eq!(req.is_public, Some(false));
    }

    #[test]
    fn test_create_playlist_request_minimal() {
        let json = r#"{"name": "Test"}"#;
        let req: CreatePlaylistRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "Test");
        assert!(req.description.is_none());
        assert!(req.is_public.is_none());
    }

    #[test]
    fn test_update_playlist_request_deserialization() {
        let json = r#"{"name": "New Name"}"#;
        let req: UpdatePlaylistRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name.as_deref(), Some("New Name"));
        assert!(req.description.is_none());
    }

    #[test]
    fn test_add_track_request() {
        let id = Uuid::new_v4();
        let json = format!(r#"{{"track_id": "{}", "position": 3}}"#, id);
        let req: AddTrackRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req.track_id, id);
        assert_eq!(req.position, Some(3));
    }
}
