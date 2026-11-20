use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
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

fn database_error(error: sea_orm::DbErr) -> (StatusCode, String) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("DB error: {error}"),
    )
}

/// GET /api/playlists (public playlists)
pub async fn list_playlists(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<super::tracks::PaginatedResponse<PlaylistResponse>>, (StatusCode, String)> {
    playlist_page(&state, params, playlist::Column::IsPublic.eq(true)).await
}

/// GET /api/me/playlists (only the authenticated member's playlists)
pub async fn my_playlists(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<AuthUser>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<super::tracks::PaginatedResponse<PlaylistResponse>>, (StatusCode, String)> {
    playlist_page(&state, params, playlist::Column::UserId.eq(user.0.sub)).await
}

async fn playlist_page(
    state: &AppState,
    params: PaginationParams,
    scope: sea_orm::sea_query::SimpleExpr,
) -> Result<Json<super::tracks::PaginatedResponse<PlaylistResponse>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 100);
    let paginator = playlist::Entity::find()
        .filter(scope)
        .order_by_desc(playlist::Column::UpdatedAt)
        .order_by_asc(playlist::Column::Id)
        .paginate(&state.db, per_page);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let playlists = paginator
        .fetch_page(page - 1)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let ids: Vec<_> = playlists.iter().map(|item| item.id).collect();
    let counts: HashMap<Uuid, i64> = if ids.is_empty() {
        HashMap::new()
    } else {
        playlist_track::Entity::find()
            .select_only()
            .column(playlist_track::Column::PlaylistId)
            .column_as(playlist_track::Column::TrackId.count(), "count")
            .filter(playlist_track::Column::PlaylistId.is_in(ids))
            .group_by(playlist_track::Column::PlaylistId)
            .into_tuple()
            .all(&state.db)
            .await
            .map_err(database_error)?
            .into_iter()
            .collect()
    };
    let data = playlists
        .into_iter()
        .map(|playlist| {
            let count = counts.get(&playlist.id).copied().unwrap_or(0) as u64;
            let mut response = PlaylistResponse::from(playlist);
            response.track_count = Some(count);
            response
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
            .filter(track::Column::Id.is_in(track_ids))
            .all(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
    };

    let mut tracks: std::collections::HashMap<_, _> =
        tracks.into_iter().map(|t| (t.id, t)).collect();
    let mut ordered_tracks: Vec<_> = pt_entries
        .into_iter()
        .filter_map(|entry| {
            let mut response = super::tracks::TrackResponse::from(tracks.remove(&entry.track_id)?);
            response.position = Some(entry.position);
            Some(response)
        })
        .collect();
    super::tracks::attach_availability(&state, &mut ordered_tracks).await?;

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

    let created = new_playlist
        .insert(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    // Dispatch plugin event (best-effort)
    if let Some(registry) = super::get_plugin_registry(&state) {
        let payload = soundtime_plugin::PlaylistCreatedPayload {
            playlist_id: created.id.to_string(),
            user_id: auth_user.0.sub.to_string(),
            name: created.name.clone(),
        };
        let payload_val = serde_json::to_value(&payload).unwrap_or_default();
        let registry = registry.clone();
        tokio::spawn(async move {
            registry.dispatch("on_playlist_created", &payload_val).await;
        });
    }

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

    let updated = active
        .update(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

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
    let txn = state.db.begin().await.map_err(database_error)?;
    let existing = playlist::Entity::find_by_id(id)
        .lock_exclusive()
        .one(&txn)
        .await
        .map_err(database_error)?
        .ok_or((StatusCode::NOT_FOUND, "Playlist not found".to_string()))?;
    if existing.user_id != auth_user.0.sub {
        return Err((StatusCode::FORBIDDEN, "Not your playlist".to_string()));
    }

    // An interrupted client may retry a successful POST. Preserve its original position.
    if playlist_track::Entity::find_by_id((id, body.track_id))
        .one(&txn)
        .await
        .map_err(database_error)?
        .is_some()
    {
        txn.commit().await.map_err(database_error)?;
        return Ok(StatusCode::NO_CONTENT);
    }

    let position = match body.position {
        Some(position) => position,
        None => playlist_track::Entity::find()
            .filter(playlist_track::Column::PlaylistId.eq(id))
            .order_by_desc(playlist_track::Column::Position)
            .one(&txn)
            .await
            .map_err(database_error)?
            .map_or(0, |last| last.position.saturating_add(1)),
    };
    playlist_track::ActiveModel {
        playlist_id: Set(id),
        track_id: Set(body.track_id),
        position: Set(position),
    }
    .insert(&txn)
    .await
    .map_err(database_error)?;
    txn.commit().await.map_err(database_error)?;

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
