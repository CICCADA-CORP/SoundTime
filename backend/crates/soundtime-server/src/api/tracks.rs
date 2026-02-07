use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use soundtime_db::entities::{album, artist, listen_history, remote_track, track};
use soundtime_db::AppState;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    pub total: u64,
    pub page: u64,
    pub per_page: u64,
    pub total_pages: u64,
}

#[derive(Debug, Serialize)]
pub struct TrackResponse {
    pub id: Uuid,
    pub title: String,
    pub artist_id: Uuid,
    pub album_id: Option<Uuid>,
    pub track_number: Option<i16>,
    pub disc_number: Option<i16>,
    pub duration_secs: f32,
    pub genre: Option<String>,
    pub year: Option<i16>,
    pub format: String,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub musicbrainz_id: Option<String>,
    pub uploaded_by: Option<Uuid>,
    pub play_count: i64,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
    /// Joined artist name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist_name: Option<String>,
    /// Joined album title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album_title: Option<String>,
    /// Album cover URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    /// Highest bitrate available across local + federated sources
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_bitrate: Option<i32>,
    /// Source providing the best bitrate ("local" or instance domain)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_source: Option<String>,
}

impl From<track::Model> for TrackResponse {
    fn from(t: track::Model) -> Self {
        Self {
            id: t.id,
            title: t.title,
            artist_id: t.artist_id,
            album_id: t.album_id,
            track_number: t.track_number,
            disc_number: t.disc_number,
            duration_secs: t.duration_secs,
            genre: t.genre,
            year: t.year,
            format: t.format,
            bitrate: t.bitrate,
            sample_rate: t.sample_rate,
            musicbrainz_id: t.musicbrainz_id,
            uploaded_by: t.uploaded_by,
            play_count: t.play_count,
            created_at: t.created_at,
            artist_name: None,
            album_title: None,
            cover_url: None,
            best_bitrate: None,
            best_source: None,
        }
    }
}

/// GET /api/tracks
pub async fn list_tracks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<TrackResponse>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);

    let paginator = track::Entity::find()
        .order_by_desc(track::Column::CreatedAt)
        .paginate(&state.db, per_page);

    let total = paginator.num_items().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    let tracks = paginator.fetch_page(page - 1).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    let total_pages = (total + per_page - 1) / per_page;

    // Batch-fetch artist and album data
    let artist_ids: Vec<Uuid> = tracks.iter().map(|t| t.artist_id).collect::<std::collections::HashSet<_>>().into_iter().collect();
    let album_ids: Vec<Uuid> = tracks.iter().filter_map(|t| t.album_id).collect::<std::collections::HashSet<_>>().into_iter().collect();

    let artists: HashMap<Uuid, artist::Model> = if !artist_ids.is_empty() {
        artist::Entity::find()
            .filter(artist::Column::Id.is_in(artist_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let albums: HashMap<Uuid, album::Model> = if !album_ids.is_empty() {
        album::Entity::find()
            .filter(album::Column::Id.is_in(album_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let data = tracks.into_iter().map(|t| {
        let artist_name = artists.get(&t.artist_id).map(|a| a.name.clone());
        let (album_title, cover_url) = t.album_id
            .and_then(|aid| albums.get(&aid))
            .map(|a| (Some(a.title.clone()), a.cover_url.clone().map(|url| {
                if url.starts_with("/api/media/") || url.starts_with("http") { url } else { format!("/api/media/{url}") }
            })))
            .unwrap_or((None, None));

        let mut resp = TrackResponse::from(t);
        resp.artist_name = artist_name;
        resp.album_title = album_title;
        resp.cover_url = cover_url;
        resp
    }).collect();

    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        per_page,
        total_pages,
    }))
}

/// GET /api/tracks/my-uploads – returns only the authenticated user's uploads
pub async fn my_uploads(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<TrackResponse>>, (StatusCode, String)> {
    let user_id = user.0.sub;
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);

    let paginator = track::Entity::find()
        .filter(track::Column::UploadedBy.eq(user_id))
        .order_by_desc(track::Column::CreatedAt)
        .paginate(&state.db, per_page);

    let total = paginator.num_items().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    let tracks = paginator.fetch_page(page - 1).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    let total_pages = (total + per_page - 1) / per_page;

    // Batch-fetch artist and album data
    let artist_ids: Vec<Uuid> = tracks.iter().map(|t| t.artist_id).collect::<std::collections::HashSet<_>>().into_iter().collect();
    let album_ids: Vec<Uuid> = tracks.iter().filter_map(|t| t.album_id).collect::<std::collections::HashSet<_>>().into_iter().collect();

    let artists: HashMap<Uuid, artist::Model> = if !artist_ids.is_empty() {
        artist::Entity::find()
            .filter(artist::Column::Id.is_in(artist_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let albums: HashMap<Uuid, album::Model> = if !album_ids.is_empty() {
        album::Entity::find()
            .filter(album::Column::Id.is_in(album_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let data = tracks.into_iter().map(|t| {
        let artist_name = artists.get(&t.artist_id).map(|a| a.name.clone());
        let (album_title, cover_url) = t.album_id
            .and_then(|aid| albums.get(&aid))
            .map(|a| (Some(a.title.clone()), a.cover_url.clone().map(|url| {
                if url.starts_with("/api/media/") || url.starts_with("http") { url } else { format!("/api/media/{url}") }
            })))
            .unwrap_or((None, None));

        let mut resp = TrackResponse::from(t);
        resp.artist_name = artist_name;
        resp.album_title = album_title;
        resp.cover_url = cover_url;
        resp
    }).collect();

    Ok(Json(PaginatedResponse {
        data,
        total,
        page,
        per_page,
        total_pages,
    }))
}

/// GET /api/tracks/:id
pub async fn get_track(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<TrackResponse>, (StatusCode, String)> {
    let track_model = track::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Track not found".to_string()))?;

    let mut resp = TrackResponse::from(track_model.clone());

    // Fetch artist name
    if let Ok(Some(a)) = artist::Entity::find_by_id(track_model.artist_id).one(&state.db).await {
        resp.artist_name = Some(a.name);
    }

    // Fetch album title & cover
    if let Some(album_id) = track_model.album_id {
        if let Ok(Some(a)) = album::Entity::find_by_id(album_id).one(&state.db).await {
            resp.album_title = Some(a.title);
            resp.cover_url = a.cover_url.map(|url| {
                if url.starts_with("/api/media/") || url.starts_with("http") { url } else { format!("/api/media/{url}") }
            });
        }
    }

    // Compute best available bitrate across local and remote sources
    let local_bitrate = track_model.bitrate.unwrap_or(0);
    let mut best_bitrate = local_bitrate;
    let mut best_source = "local".to_string();

    // Check remote tracks linked to this one
    let remotes = remote_track::Entity::find()
        .filter(remote_track::Column::LocalTrackId.eq(Some(id)))
        .filter(remote_track::Column::IsAvailable.eq(true))
        .all(&state.db)
        .await
        .unwrap_or_default();

    for rt in &remotes {
        if rt.bitrate.unwrap_or(0) > best_bitrate {
            best_bitrate = rt.bitrate.unwrap_or(0);
            best_source = rt.instance_domain.clone();
        }
    }

    // Also check by musicbrainz_id
    if let Some(ref mbid) = track_model.musicbrainz_id {
        let remotes_mb = remote_track::Entity::find()
            .filter(remote_track::Column::MusicbrainzId.eq(mbid.clone()))
            .filter(remote_track::Column::IsAvailable.eq(true))
            .all(&state.db)
            .await
            .unwrap_or_default();

        for rt in &remotes_mb {
            if rt.bitrate.unwrap_or(0) > best_bitrate {
                best_bitrate = rt.bitrate.unwrap_or(0);
                best_source = rt.instance_domain.clone();
            }
        }
    }

    if best_bitrate > 0 {
        resp.best_bitrate = Some(best_bitrate);
        resp.best_source = Some(best_source);
    }

    Ok(Json(resp))
}

// ─── Track Credits / Metadata (public) ──────────────────────────────

#[derive(Debug, Serialize)]
pub struct TrackCreditsResponse {
    pub id: Uuid,
    pub title: String,
    pub duration_secs: f32,
    pub format: String,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub genre: Option<String>,
    pub year: Option<i16>,
    pub track_number: Option<i16>,
    pub disc_number: Option<i16>,
    pub musicbrainz_id: Option<String>,
    pub play_count: i64,
    pub uploaded_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
    // Artist info
    pub artist_id: Uuid,
    pub artist_name: String,
    pub artist_bio: Option<String>,
    pub artist_image: Option<String>,
    pub artist_musicbrainz_id: Option<String>,
    // Album info
    pub album_id: Option<Uuid>,
    pub album_title: Option<String>,
    pub album_cover_url: Option<String>,
    pub album_genre: Option<String>,
    pub album_year: Option<i16>,
    pub album_musicbrainz_id: Option<String>,
    // Best source
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_bitrate: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_source: Option<String>,
}

/// GET /api/tracks/:id/credits — full metadata & credits (public)
pub async fn get_track_credits(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<TrackCreditsResponse>, (StatusCode, String)> {
    let track_model = track::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Track not found".to_string()))?;

    let artist_model = artist::Entity::find_by_id(track_model.artist_id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Artist not found".to_string()))?;

    let album_model = if let Some(album_id) = track_model.album_id {
        album::Entity::find_by_id(album_id)
            .one(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
    } else {
        None
    };

    Ok(Json(TrackCreditsResponse {
        id: track_model.id,
        title: track_model.title,
        duration_secs: track_model.duration_secs,
        format: track_model.format,
        bitrate: track_model.bitrate,
        sample_rate: track_model.sample_rate,
        genre: track_model.genre,
        year: track_model.year,
        track_number: track_model.track_number,
        disc_number: track_model.disc_number,
        musicbrainz_id: track_model.musicbrainz_id,
        play_count: track_model.play_count,
        uploaded_by: track_model.uploaded_by,
        created_at: track_model.created_at,
        artist_id: artist_model.id,
        artist_name: artist_model.name,
        artist_bio: artist_model.bio,
        artist_image: artist_model.image_url,
        artist_musicbrainz_id: artist_model.musicbrainz_id,
        album_id: album_model.as_ref().map(|a| a.id),
        album_title: album_model.as_ref().map(|a| a.title.clone()),
        album_cover_url: album_model.as_ref().and_then(|a| a.cover_url.clone()),
        album_genre: album_model.as_ref().and_then(|a| a.genre.clone()),
        album_year: album_model.as_ref().and_then(|a| a.year),
        album_musicbrainz_id: album_model.as_ref().and_then(|a| a.musicbrainz_id.clone()),
        best_bitrate: None,
        best_source: None,
    }))
}

// ─── Track Update (owner only) ──────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateTrackRequest {
    pub title: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i16>,
    pub track_number: Option<i16>,
    pub disc_number: Option<i16>,
}

/// PUT /api/tracks/:id — update track metadata (owner only)
pub async fn update_track(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateTrackRequest>,
) -> Result<Json<TrackResponse>, (StatusCode, String)> {
    let existing = track::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Track not found".to_string()))?;

    // Only the uploader can edit
    match existing.uploaded_by {
        Some(uploader_id) if uploader_id == user.0.sub => {}
        _ => return Err((StatusCode::FORBIDDEN, "You can only edit your own tracks".to_string())),
    }

    let mut active: track::ActiveModel = existing.into();
    if let Some(title) = body.title {
        active.title = Set(title);
    }
    if let Some(genre) = body.genre {
        active.genre = Set(Some(genre));
    }
    if let Some(year) = body.year {
        active.year = Set(Some(year));
    }
    if let Some(track_number) = body.track_number {
        active.track_number = Set(Some(track_number));
    }
    if let Some(disc_number) = body.disc_number {
        active.disc_number = Set(Some(disc_number));
    }

    let updated = active.update(&state.db).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    Ok(Json(TrackResponse::from(updated)))
}

/// DELETE /api/tracks/:id — delete track (owner only)
pub async fn delete_track(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    let existing = track::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Track not found".to_string()))?;

    // Only the uploader can delete
    match existing.uploaded_by {
        Some(uploader_id) if uploader_id == user.0.sub => {}
        _ => return Err((StatusCode::FORBIDDEN, "You can only delete your own tracks".to_string())),
    }

    // Delete the audio file
    let _ = state.storage.delete_file(&existing.file_path).await;

    // Remove from playlists, favorites, history
    use soundtime_db::entities::{favorite, playlist_track};
    playlist_track::Entity::delete_many()
        .filter(playlist_track::Column::TrackId.eq(id))
        .exec(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    favorite::Entity::delete_many()
        .filter(favorite::Column::TrackId.eq(id))
        .exec(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    listen_history::Entity::delete_many()
        .filter(listen_history::Column::TrackId.eq(id))
        .exec(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    // Delete the track itself
    track::Entity::delete_by_id(id)
        .exec(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── Explore: Popular Tracks ────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ExploreParams {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

/// GET /api/tracks/popular — tracks sorted by play_count DESC
pub async fn list_popular_tracks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExploreParams>,
) -> Result<Json<PaginatedResponse<TrackResponse>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);

    let paginator = track::Entity::find()
        .order_by_desc(track::Column::PlayCount)
        .paginate(&state.db, per_page);

    let total = paginator.num_items().await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    let tracks = paginator.fetch_page(page - 1).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}"))
    })?;

    let total_pages = (total + per_page - 1) / per_page;

    Ok(Json(PaginatedResponse {
        data: tracks.into_iter().map(TrackResponse::from).collect(),
        total,
        page,
        per_page,
        total_pages,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_track_model() -> track::Model {
        track::Model {
            id: Uuid::new_v4(),
            title: "Test Track".into(),
            artist_id: Uuid::new_v4(),
            album_id: Some(Uuid::new_v4()),
            track_number: Some(1),
            disc_number: Some(1),
            duration_secs: 240.5,
            genre: Some("Rock".into()),
            year: Some(2024),
            musicbrainz_id: None,
            file_path: "/data/music/test.mp3".into(),
            file_size: 5_000_000,
            format: "mp3".into(),
            bitrate: Some(320),
            sample_rate: Some(44100),
            waveform_data: None,
            uploaded_by: Some(Uuid::new_v4()),
            play_count: 42,
            content_hash: None,
            created_at: Utc::now().fixed_offset(),
        }
    }

    #[test]
    fn test_track_response_from_model() {
        let model = make_track_model();
        let id = model.id;
        let resp = TrackResponse::from(model);
        assert_eq!(resp.id, id);
        assert_eq!(resp.title, "Test Track");
        assert_eq!(resp.play_count, 42);
        assert!(resp.artist_name.is_none());
        assert!(resp.album_title.is_none());
        assert!(resp.cover_url.is_none());
    }

    #[test]
    fn test_track_response_serialization() {
        let model = make_track_model();
        let resp = TrackResponse::from(model);
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["title"], "Test Track");
        assert_eq!(json["format"], "mp3");
        assert_eq!(json["play_count"], 42);
        // Optional None fields with skip_serializing_if should be absent
        assert!(json.get("artist_name").is_none());
        assert!(json.get("album_title").is_none());
    }

    #[test]
    fn test_paginated_response_serialization() {
        let resp = PaginatedResponse {
            data: vec!["a".to_string(), "b".to_string()],
            total: 10,
            page: 1,
            per_page: 2,
            total_pages: 5,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["total"], 10);
        assert_eq!(json["page"], 1);
        assert_eq!(json["per_page"], 2);
        assert_eq!(json["total_pages"], 5);
        assert_eq!(json["data"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_pagination_params_defaults() {
        let params: PaginationParams = serde_json::from_str("{}").unwrap();
        assert!(params.page.is_none());
        assert!(params.per_page.is_none());
    }

    #[test]
    fn test_pagination_params_custom() {
        let params: PaginationParams =
            serde_json::from_str(r#"{"page": 3, "per_page": 50}"#).unwrap();
        assert_eq!(params.page, Some(3));
        assert_eq!(params.per_page, Some(50));
    }
}
