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
pub struct ArtistResponse {
    pub id: Uuid,
    pub name: String,
    pub musicbrainz_id: Option<String>,
    pub bio: Option<String>,
    pub image_url: Option<String>,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
}

impl From<artist::Model> for ArtistResponse {
    fn from(a: artist::Model) -> Self {
        Self {
            id: a.id,
            name: a.name,
            musicbrainz_id: a.musicbrainz_id,
            bio: a.bio,
            image_url: a.image_url,
            created_at: a.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ArtistDetailResponse {
    #[serde(flatten)]
    pub artist: ArtistResponse,
    pub albums: Vec<super::albums::AlbumResponse>,
    pub tracks: Vec<super::tracks::TrackResponse>,
}

/// GET /api/artists
pub async fn list_artists(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<super::tracks::PaginatedResponse<ArtistResponse>>, (StatusCode, String)> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).min(100);

    let paginator = artist::Entity::find()
        .order_by_asc(artist::Column::Name)
        .paginate(&state.db, per_page);

    let total = paginator
        .num_items()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let artists = paginator
        .fetch_page(page - 1)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_pages = (total + per_page - 1) / per_page;

    Ok(Json(super::tracks::PaginatedResponse {
        data: artists.into_iter().map(ArtistResponse::from).collect(),
        total,
        page,
        per_page,
        total_pages,
    }))
}

/// GET /api/artists/:id
pub async fn get_artist(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<ArtistDetailResponse>, (StatusCode, String)> {
    let artist_model = artist::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Artist not found".to_string()))?;

    let albums = album::Entity::find()
        .filter(album::Column::ArtistId.eq(id))
        .order_by_desc(album::Column::Year)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let tracks = track::Entity::find()
        .filter(track::Column::ArtistId.eq(id))
        .order_by_desc(track::Column::CreatedAt)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(ArtistDetailResponse {
        artist: ArtistResponse::from(artist_model.clone()),
        albums: albums
            .into_iter()
            .map(|a| super::albums::AlbumResponse::from_model(a, Some(artist_model.name.clone())))
            .collect(),
        tracks: tracks
            .into_iter()
            .map(|t| {
                let mut resp = super::tracks::TrackResponse::from(t);
                resp.artist_name = Some(artist_model.name.clone());
                resp
            })
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_artist_model() -> artist::Model {
        artist::Model {
            id: Uuid::new_v4(),
            name: "Test Artist".into(),
            musicbrainz_id: Some("mb-12345".into()),
            bio: Some("A great artist".into()),
            image_url: Some("https://img.example.com/artist.jpg".into()),
            created_at: Utc::now().fixed_offset(),
        }
    }

    #[test]
    fn test_artist_response_from_model() {
        let model = make_artist_model();
        let id = model.id;
        let resp = ArtistResponse::from(model);
        assert_eq!(resp.id, id);
        assert_eq!(resp.name, "Test Artist");
        assert_eq!(resp.musicbrainz_id.as_deref(), Some("mb-12345"));
        assert_eq!(resp.bio.as_deref(), Some("A great artist"));
    }

    #[test]
    fn test_artist_response_serialization() {
        let model = make_artist_model();
        let resp = ArtistResponse::from(model);
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["name"], "Test Artist");
        assert!(json["image_url"].is_string());
    }

    #[test]
    fn test_artist_response_no_optional_fields() {
        let model = artist::Model {
            id: Uuid::new_v4(),
            name: "Minimal Artist".into(),
            musicbrainz_id: None,
            bio: None,
            image_url: None,
            created_at: Utc::now().fixed_offset(),
        };
        let resp = ArtistResponse::from(model);
        assert!(resp.musicbrainz_id.is_none());
        assert!(resp.bio.is_none());
        assert!(resp.image_url.is_none());
    }
}
