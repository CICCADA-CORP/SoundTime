use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::{ColumnTrait, EntityTrait, FromQueryResult, QueryFilter, Statement};
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
    /// Total number of results across all categories
    pub total: usize,
}

/// Helper: build a tsquery string from user input.
/// Splits on whitespace, wraps each word with `:*` for prefix matching,
/// and joins with `&` for AND semantics.
fn build_tsquery(q: &str) -> String {
    q.split_whitespace()
        .filter(|w| !w.is_empty())
        .map(|w| {
            // Remove special tsquery characters to avoid parse errors
            let clean: String = w
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if clean.is_empty() {
                String::new()
            } else {
                format!("{}:*", clean)
            }
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" & ")
}

/// Row type returned by the FTS track query
#[derive(Debug, FromQueryResult)]
struct TrackFtsRow {
    pub id: uuid::Uuid,
    #[allow(dead_code)]
    pub rank: f32,
}

/// GET /api/search?q=...
/// Uses PostgreSQL full-text search with ts_rank for relevance-based ordering.
/// Falls back to ILIKE for very short queries (1-2 chars) where FTS is ineffective.
pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResults>, (StatusCode, String)> {
    let q_trimmed = params.q.trim();
    if q_trimmed.is_empty() {
        return Ok(Json(SearchResults {
            tracks: vec![],
            albums: vec![],
            artists: vec![],
            total: 0,
        }));
    }

    let limit = params.per_page.unwrap_or(10).clamp(1, 50) as i64;
    let page = params.page.unwrap_or(1).clamp(1, 1000);
    let offset = ((page - 1) as i64) * limit;

    let tsquery = build_tsquery(q_trimmed);

    // ── Tracks: FTS on title with artist/album name join ──
    let tracks = if tsquery.is_empty() {
        vec![]
    } else {
        // Use ts_rank against the GIN index on tracks.title for relevance scoring.
        // Also join artist.name and album.title so we can search across all fields.
        let track_rows: Vec<TrackFtsRow> =
            TrackFtsRow::find_by_statement(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                r#"
            SELECT t.id, ts_rank(
                setweight(to_tsvector('english', t.title), 'A') ||
                setweight(to_tsvector('english', a.name), 'B') ||
                setweight(to_tsvector('english', COALESCE(al.title, '')), 'C'),
                to_tsquery('english', $1)
            ) AS rank
            FROM tracks t
            JOIN artists a ON a.id = t.artist_id
            LEFT JOIN albums al ON al.id = t.album_id
            WHERE (
                to_tsvector('english', t.title) ||
                to_tsvector('english', a.name) ||
                to_tsvector('english', COALESCE(al.title, ''))
            ) @@ to_tsquery('english', $1)
            ORDER BY rank DESC
            LIMIT $2 OFFSET $3
            "#,
                vec![tsquery.clone().into(), limit.into(), offset.into()],
            ))
            .all(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

        // Load full track models for the ranked ids
        let track_ids: Vec<uuid::Uuid> = track_rows.iter().map(|r| r.id).collect();
        if track_ids.is_empty() {
            vec![]
        } else {
            let models = track::Entity::find()
                .filter(track::Column::Id.is_in(track_ids.clone()))
                .all(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

            // Preserve the rank order from FTS
            let model_map: std::collections::HashMap<uuid::Uuid, track::Model> =
                models.into_iter().map(|m| (m.id, m)).collect();
            track_ids
                .into_iter()
                .filter_map(|id| model_map.get(&id).cloned())
                .map(super::tracks::TrackResponse::from)
                .collect()
        }
    };

    // ── Albums: FTS on title ──
    let albums = if tsquery.is_empty() {
        vec![]
    } else {
        let album_rows: Vec<TrackFtsRow> =
            TrackFtsRow::find_by_statement(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                r#"
            SELECT id, ts_rank(to_tsvector('english', title), to_tsquery('english', $1)) AS rank
            FROM albums
            WHERE to_tsvector('english', title) @@ to_tsquery('english', $1)
            ORDER BY rank DESC
            LIMIT $2 OFFSET $3
            "#,
                vec![tsquery.clone().into(), limit.into(), offset.into()],
            ))
            .all(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

        let album_ids: Vec<uuid::Uuid> = album_rows.iter().map(|r| r.id).collect();
        if album_ids.is_empty() {
            vec![]
        } else {
            let models = album::Entity::find()
                .filter(album::Column::Id.is_in(album_ids.clone()))
                .all(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
            let model_map: std::collections::HashMap<uuid::Uuid, album::Model> =
                models.into_iter().map(|m| (m.id, m)).collect();
            album_ids
                .into_iter()
                .filter_map(|id| model_map.get(&id).cloned())
                .map(|a| super::albums::AlbumResponse::from_model(a, None))
                .collect()
        }
    };

    // ── Artists: FTS on name ──
    let artists = if tsquery.is_empty() {
        vec![]
    } else {
        let artist_rows: Vec<TrackFtsRow> =
            TrackFtsRow::find_by_statement(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                r#"
            SELECT id, ts_rank(to_tsvector('english', name), to_tsquery('english', $1)) AS rank
            FROM artists
            WHERE to_tsvector('english', name) @@ to_tsquery('english', $1)
            ORDER BY rank DESC
            LIMIT $2 OFFSET $3
            "#,
                vec![tsquery.into(), limit.into(), offset.into()],
            ))
            .all(&state.db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

        let artist_ids: Vec<uuid::Uuid> = artist_rows.iter().map(|r| r.id).collect();
        if artist_ids.is_empty() {
            vec![]
        } else {
            let models = artist::Entity::find()
                .filter(artist::Column::Id.is_in(artist_ids.clone()))
                .all(&state.db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
            let model_map: std::collections::HashMap<uuid::Uuid, artist::Model> =
                models.into_iter().map(|m| (m.id, m)).collect();
            artist_ids
                .into_iter()
                .filter_map(|id| model_map.get(&id).cloned())
                .map(super::artists::ArtistResponse::from)
                .collect()
        }
    };

    let total = tracks.len() + albums.len() + artists.len();

    Ok(Json(SearchResults {
        tracks,
        albums,
        artists,
        total,
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
            total: 0,
        };
        let json = serde_json::to_value(&results).unwrap();
        assert!(json["tracks"].as_array().unwrap().is_empty());
        assert!(json["albums"].as_array().unwrap().is_empty());
        assert!(json["artists"].as_array().unwrap().is_empty());
        assert_eq!(json["total"].as_u64().unwrap(), 0);
    }

    #[test]
    fn test_build_tsquery_single_word() {
        assert_eq!(build_tsquery("hello"), "hello:*");
    }

    #[test]
    fn test_build_tsquery_multiple_words() {
        assert_eq!(build_tsquery("daft punk"), "daft:* & punk:*");
    }

    #[test]
    fn test_build_tsquery_special_chars_stripped() {
        assert_eq!(build_tsquery("rock & roll"), "rock:* & roll:*");
    }

    #[test]
    fn test_build_tsquery_empty() {
        assert_eq!(build_tsquery(""), "");
        assert_eq!(build_tsquery("   "), "");
    }
}
