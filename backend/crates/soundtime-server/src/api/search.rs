use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use soundtime_db::entities::{album, artist, track};
use soundtime_db::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
    #[allow(dead_code)]
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct SearchResults {
    pub tracks: Vec<super::tracks::TrackResponse>,
    pub albums: Vec<super::albums::AlbumResponse>,
    pub artists: Vec<super::artists::ArtistResponse>,
}

/// GET /api/search?q=...
pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResults>, (StatusCode, String)> {
    let q_trimmed = params.q.trim();
    // SECURITY: escape SQL LIKE wildcards to prevent wildcard-abuse DoS
    let q_escaped = q_trimmed.replace('%', "\\%").replace('_', "\\_");
    let query = format!("%{}%", q_escaped);
    let limit = params.per_page.unwrap_or(10).min(50);

    let tracks = track::Entity::find()
        .filter(track::Column::Title.like(&query))
        .order_by_desc(track::Column::CreatedAt)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .into_iter()
        .take(limit as usize)
        .map(super::tracks::TrackResponse::from)
        .collect();

    let albums = album::Entity::find()
        .filter(album::Column::Title.like(&query))
        .order_by_desc(album::Column::CreatedAt)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .into_iter()
        .take(limit as usize)
        .map(|a| super::albums::AlbumResponse::from_model(a, None))
        .collect();

    let artists = artist::Entity::find()
        .filter(artist::Column::Name.like(&query))
        .order_by_asc(artist::Column::Name)
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .into_iter()
        .take(limit as usize)
        .map(super::artists::ArtistResponse::from)
        .collect();

    Ok(Json(SearchResults {
        tracks,
        albums,
        artists,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_params_deserialization() {
        let json = r#"{"q": "love", "page": 2, "per_page": 25}"#;
        let params: SearchParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.q, "love");
        assert_eq!(params.page, Some(2));
        assert_eq!(params.per_page, Some(25));
    }

    #[test]
    fn test_search_params_minimal() {
        let json = r#"{"q": "test"}"#;
        let params: SearchParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.q, "test");
        assert!(params.page.is_none());
        assert!(params.per_page.is_none());
    }

    #[test]
    fn test_search_results_serialization() {
        let results = SearchResults {
            tracks: vec![],
            albums: vec![],
            artists: vec![],
        };
        let json = serde_json::to_value(&results).unwrap();
        assert!(json["tracks"].as_array().unwrap().is_empty());
        assert!(json["albums"].as_array().unwrap().is_empty());
        assert!(json["artists"].as_array().unwrap().is_empty());
    }
}
