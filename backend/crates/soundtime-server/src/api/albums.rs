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
use soundtime_db::entities::{album, artist, track};
use soundtime_db::AppState;

#[derive(Debug, Serialize)]
pub struct AlbumResponse {
    pub id: Uuid,
    pub title: String,
    pub artist_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist_name: Option<String>,
    pub release_date: Option<chrono::NaiveDate>,
    pub cover_url: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i16>,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
}

impl AlbumResponse {
    pub fn from_model(a: album::Model, artist_name: Option<String>) -> Self {
        let cover_url = a.cover_url.map(|url| {
            if url.starts_with("/api/media/") || url.starts_with("http") {
                url
            } else {
                format!("/api/media/{url}")
            }
        });
        Self {
            id: a.id,
            title: a.title,
            artist_id: a.artist_id,
            artist_name,
            release_date: a.release_date,
            cover_url,
            genre: a.genre,
            year: a.year,
            created_at: a.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AlbumDetailResponse {
    #[serde(flatten)]
    pub album: AlbumResponse,
    pub tracks: Vec<super::tracks::TrackResponse>,
}

/// GET /api/albums
pub async fn list_albums(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<super::tracks::PaginatedResponse<AlbumResponse>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);

    let paginator = album::Entity::find()
        .order_by_desc(album::Column::CreatedAt)
        .paginate(&state.db, per_page);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let albums = paginator
        .fetch_page(page - 1)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_pages = total.div_ceil(per_page);

    // Batch-fetch artist names
    let artist_ids: Vec<Uuid> = albums
        .iter()
        .map(|a| a.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let artists: std::collections::HashMap<Uuid, String> = if !artist_ids.is_empty() {
        artist::Entity::find()
            .filter(artist::Column::Id.is_in(artist_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a.name))
            .collect()
    } else {
        std::collections::HashMap::new()
    };

    Ok(Json(super::tracks::PaginatedResponse {
        data: albums
            .into_iter()
            .map(|a| {
                let name = artists.get(&a.artist_id).cloned();
                AlbumResponse::from_model(a, name)
            })
            .collect(),
        total,
        page,
        per_page,
        total_pages,
    }))
}

/// GET /api/albums/:id
pub async fn get_album(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<AlbumDetailResponse>, (StatusCode, String)> {
    let album_model = album::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Album not found".to_string()))?;

    // Fetch artist name
    let artist_name = artist::Entity::find_by_id(album_model.artist_id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .map(|a| a.name);

    let tracks = track::Entity::find()
        .filter(track::Column::AlbumId.eq(id))
        .order_by_asc(track::Column::DiscNumber)
        .order_by_asc(track::Column::TrackNumber)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    // Build cover_url for the tracks from the album
    let album_cover = album_model.cover_url.as_ref().map(|url| {
        if url.starts_with("/api/media/") || url.starts_with("http") {
            url.clone()
        } else {
            format!("/api/media/{url}")
        }
    });

    let enriched_tracks = tracks
        .into_iter()
        .map(|t| {
            let mut resp = super::tracks::TrackResponse::from(t);
            resp.artist_name = artist_name.clone();
            resp.album_title = Some(album_model.title.clone());
            resp.cover_url = album_cover.clone();
            resp
        })
        .collect();

    Ok(Json(AlbumDetailResponse {
        album: AlbumResponse::from_model(album_model, artist_name),
        tracks: enriched_tracks,
    }))
}

/// GET /api/albums/recent — albums sorted by created_at DESC
pub async fn list_recent_albums(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<super::tracks::PaginatedResponse<AlbumResponse>>, (StatusCode, String)> {
    let per_page = params.per_page.unwrap_or(10).min(50);

    let albums = album::Entity::find()
        .order_by_desc(album::Column::CreatedAt)
        .paginate(&state.db, per_page);

    let total = albums
        .num_items()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let album_list = albums
        .fetch_page(0)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_pages = total.div_ceil(per_page);

    // Batch-fetch artist names
    let artist_ids: Vec<Uuid> = album_list
        .iter()
        .map(|a| a.artist_id)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let artists: std::collections::HashMap<Uuid, String> = if !artist_ids.is_empty() {
        artist::Entity::find()
            .filter(artist::Column::Id.is_in(artist_ids))
            .all(&state.db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a.name))
            .collect()
    } else {
        std::collections::HashMap::new()
    };

    Ok(Json(super::tracks::PaginatedResponse {
        data: album_list
            .into_iter()
            .map(|a| {
                let name = artists.get(&a.artist_id).cloned();
                AlbumResponse::from_model(a, name)
            })
            .collect(),
        total,
        page: 1,
        per_page,
        total_pages,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_album_model() -> album::Model {
        album::Model {
            id: Uuid::new_v4(),
            title: "Test Album".into(),
            artist_id: Uuid::new_v4(),
            release_date: None,
            cover_url: Some("covers/test.jpg".into()),
            musicbrainz_id: None,
            genre: Some("Rock".into()),
            year: Some(2024),
            created_at: Utc::now().fixed_offset(),
        }
    }

    #[test]
    fn test_album_response_from_model() {
        let model = make_album_model();
        let id = model.id;
        let resp = AlbumResponse::from_model(model, Some("The Artist".into()));
        assert_eq!(resp.id, id);
        assert_eq!(resp.title, "Test Album");
        assert_eq!(resp.artist_name.as_deref(), Some("The Artist"));
    }

    #[test]
    fn test_album_cover_url_prepend_api() {
        let model = make_album_model();
        let resp = AlbumResponse::from_model(model, None);
        assert_eq!(
            resp.cover_url.as_deref(),
            Some("/api/media/covers/test.jpg")
        );
    }

    #[test]
    fn test_album_cover_url_keeps_api_prefix() {
        let mut model = make_album_model();
        model.cover_url = Some("/api/media/covers/test.jpg".into());
        let resp = AlbumResponse::from_model(model, None);
        assert_eq!(
            resp.cover_url.as_deref(),
            Some("/api/media/covers/test.jpg")
        );
    }

    #[test]
    fn test_album_cover_url_keeps_http() {
        let mut model = make_album_model();
        model.cover_url = Some("https://example.com/cover.jpg".into());
        let resp = AlbumResponse::from_model(model, None);
        assert_eq!(
            resp.cover_url.as_deref(),
            Some("https://example.com/cover.jpg")
        );
    }

    #[test]
    fn test_album_cover_url_none() {
        let mut model = make_album_model();
        model.cover_url = None;
        let resp = AlbumResponse::from_model(model, None);
        assert!(resp.cover_url.is_none());
    }

    #[test]
    fn test_album_response_serialization() {
        let model = make_album_model();
        let resp = AlbumResponse::from_model(model, None);
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["title"], "Test Album");
        assert_eq!(json["year"], 2024);
    }
}
