use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use futures_util::{
    future::{BoxFuture, WeakShared},
    FutureExt,
};
use sea_orm::{ConnectionTrait, DbBackend, DbErr, FromQueryResult, Statement};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use uuid::Uuid;

use soundtime_db::entities::{album, artist, track};
use soundtime_db::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
    pub page: Option<u64>,
    pub per_page: Option<u64>,
    /// When true, also query P2P peers via distributed search
    #[serde(default)]
    pub include_p2p: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct SearchResults {
    pub tracks: Vec<super::tracks::TrackResponse>,
    pub albums: Vec<super::albums::AlbumResponse>,
    pub artists: Vec<super::artists::ArtistResponse>,
    /// Total number of results across all categories
    pub total: usize,
    /// P2P search results from remote peers (only present when include_p2p=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p2p_results: Option<Vec<RemoteSearchHit>>,
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

#[derive(Debug, Serialize)]
pub struct RemoteSearchHit {
    #[serde(flatten)]
    item: soundtime_p2p::SearchResultItem,
    #[serde(skip_serializing_if = "Option::is_none")]
    track_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    track: Option<super::tracks::TrackResponse>,
}

type SearchKey = (String, String, u32);
type RawResults = Option<Arc<Vec<soundtime_p2p::SearchResultItem>>>;
type PendingSearch = WeakShared<BoxFuture<'static, RawResults>>;
// Weak handles keep no transfer alive when every waiting HTTP client leaves.
static SEARCHES: LazyLock<Mutex<HashMap<SearchKey, PendingSearch>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn database_error(error: DbErr) -> (StatusCode, String) {
    tracing::error!(%error, "catalogue search failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Catalogue search unavailable".into(),
    )
}

pub async fn network_results(
    state: &AppState,
    node: Arc<soundtime_p2p::P2pNode>,
    query: &str,
    limit: u32,
) -> Result<Vec<RemoteSearchHit>, (StatusCode, String)> {
    if query.is_empty() || limit == 0 {
        return Ok(vec![]);
    }
    if query.len() > 512 {
        return Err((StatusCode::BAD_REQUEST, "Search query is too long".into()));
    }
    let key = (node.node_id().to_string(), query.to_owned(), limit);
    let strict = node.explicit_catalog();
    let flight = {
        let mut pending = SEARCHES.lock().expect("network search lock");
        pending.retain(|_, value| value.upgrade().is_some());
        if let Some(existing) = pending.get(&key).and_then(WeakShared::upgrade) {
            existing
        } else {
            if pending.len() >= 16 {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    "Network search is busy".into(),
                ));
            }
            let query = query.to_owned();
            let shared = async move {
                tokio::time::timeout(
                    Duration::from_secs(4),
                    node.distributed_search(&query, limit),
                )
                .await
                .ok()
                .map(Arc::new)
            }
            .boxed()
            .shared();
            pending.insert(key, shared.downgrade().expect("new search future"));
            shared
        }
    };
    let raw = flight.await.ok_or((
        StatusCode::GATEWAY_TIMEOUT,
        "Network search timed out".into(),
    ))?;
    if raw.is_empty() {
        return Ok(vec![]);
    }
    // Recheck mutable policy after the shared transfer and before exposing results.
    // The server only attaches an actionable UUID if this provider has one unique
    // locally known publication matching the offered version.
    let ids: Vec<_> = raw
        .iter()
        .take(limit as usize)
        .enumerate()
        .map(|(ordinal,r)| serde_json::json!({
            "ordinal": ordinal, "hash": r.hash, "source_node": r.source_node,
            "authority_node": r.authority_node, "publication_id": r.publication_id, "revision": r.revision,
        }))
        .collect();
    let stmt = Statement::from_sql_and_values(
        DbBackend::Postgres,
        include_str!("search/network_results.sql"),
        [serde_json::Value::Array(ids).into(), strict.into()],
    );
    let rows = state.db.query_all(stmt).await.map_err(database_error)?;
    let mut hits = Vec::with_capacity(rows.len());
    for row in rows {
        let ordinal: i32 = row.try_get("", "ordinal").map_err(database_error)?;
        let track_id: Option<Uuid> = row.try_get("", "track_id").map_err(database_error)?;
        let item = raw
            .get(ordinal as usize)
            .ok_or_else(|| database_error(DbErr::Custom("invalid search result index".into())))?;
        let track = track_id.map(|_| track_hit(&row)).transpose()?;
        hits.push(RemoteSearchHit {
            track,
            item: item.clone(),
            track_id,
        });
    }
    Ok(hits)
}

fn track_hit(
    row: &sea_orm::QueryResult,
) -> Result<super::tracks::TrackResponse, (StatusCode, String)> {
    let model = track::Model::from_query_result(row, "").map_err(database_error)?;
    let mut hit = super::tracks::TrackResponse::playable(model);
    hit.artist_name = row
        .try_get("", "result_artist_name")
        .map_err(database_error)?;
    hit.album_title = row
        .try_get("", "result_album_title")
        .map_err(database_error)?;
    let cover: Option<String> = row
        .try_get("", "result_cover_url")
        .map_err(database_error)?;
    hit.cover_url = cover.map(|url| {
        if url.starts_with("/api/media/") || url.starts_with("http") {
            url
        } else {
            format!("/api/media/{url}")
        }
    });
    Ok(hit)
}

/// GET /api/search searches indexed catalogues. The separate /api/p2p/search
/// route serves explicit wider searches. Older clients can request both with
/// include_p2p=true; a slow or unavailable peer cannot fail their local results.
pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<SearchResults>, (StatusCode, String)> {
    let q = params.q.trim();
    if q.len() > 512 {
        return Err((StatusCode::BAD_REQUEST, "Search query is too long".into()));
    }
    let terms = build_tsquery(q);
    let mut result = SearchResults {
        tracks: vec![],
        albums: vec![],
        artists: vec![],
        total: 0,
        p2p_results: None,
    };
    if terms.is_empty() {
        return Ok(Json(result));
    }

    let node = state
        .p2p
        .as_ref()
        .and_then(|n| n.clone().downcast::<soundtime_p2p::P2pNode>().ok());
    let strict = node.as_ref().is_some_and(|n| n.explicit_catalog());
    let limit = params.per_page.unwrap_or(10).clamp(1, 50) as i64;
    let offset = (params.page.unwrap_or(1).clamp(1, 1000) as i64 - 1) * limit;
    let statement = |sql: &'static str| {
        Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            [
                terms.as_str().into(),
                limit.into(),
                offset.into(),
                strict.into(),
                node.is_some().into(),
            ],
        )
    };

    // Each category is one bounded query; artist/album display names are read
    // in the same statement as eligibility, ordering, and track metadata.
    for row in state
        .db
        .query_all(statement(include_str!("search/tracks.sql")))
        .await
        .map_err(database_error)?
    {
        result.tracks.push(track_hit(&row)?);
    }
    for row in state
        .db
        .query_all(statement(include_str!("search/albums.sql")))
        .await
        .map_err(database_error)?
    {
        let model = album::Model::from_query_result(&row, "").map_err(database_error)?;
        let name = row
            .try_get("", "result_artist_name")
            .map_err(database_error)?;
        result
            .albums
            .push(super::albums::AlbumResponse::from_model(model, name));
    }
    let artists = artist::Model::find_by_statement(statement(include_str!("search/artists.sql")))
        .all(&state.db)
        .await
        .map_err(database_error)?;
    result.artists = artists
        .into_iter()
        .map(super::artists::ArtistResponse::from)
        .collect();
    result.total = result.tracks.len() + result.albums.len() + result.artists.len();
    if params.include_p2p.unwrap_or(false) {
        if let Some(node) = node {
            if let Ok(hits) = network_results(&state, node, q, limit.min(20) as u32).await {
                if !hits.is_empty() {
                    result.p2p_results = Some(hits);
                }
            }
        }
    }
    Ok(Json(result))
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
            p2p_results: None,
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
