use axum::{extract::State, http::StatusCode, Json};
use sea_orm::{EntityTrait, PaginatorTrait};
use serde::Serialize;
use std::sync::Arc;

use soundtime_db::entities::{album, artist, track};
use soundtime_db::AppState;

#[derive(Debug, Serialize)]
pub struct StatsOverview {
    pub total_tracks: u64,
    pub total_albums: u64,
    pub total_artists: u64,
    pub total_genres: u64,
    pub total_duration_secs: f64,
    pub total_peers: u64,
}

/// GET /api/stats/overview — public stats about the instance
pub async fn stats_overview(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatsOverview>, (StatusCode, String)> {
    let total_tracks = track::Entity::find()
        .count(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_albums = album::Entity::find()
        .count(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_artists = artist::Entity::find()
        .count(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    // Count distinct non-empty genres
    use sea_orm::sea_query::Expr;
    use sea_orm::{FromQueryResult, QuerySelect};

    #[derive(Debug, FromQueryResult)]
    struct CountResult {
        count: Option<i64>,
    }

    let genre_result = track::Entity::find()
        .select_only()
        .column_as(
            Expr::cust(
                "COUNT(DISTINCT CASE WHEN genre IS NOT NULL AND genre != '' THEN genre END)",
            ),
            "count",
        )
        .into_model::<CountResult>()
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_genres = genre_result.and_then(|r| r.count).unwrap_or(0) as u64;

    // Sum all durations
    #[derive(Debug, FromQueryResult)]
    struct SumResult {
        total: Option<f64>,
    }

    let duration_result = track::Entity::find()
        .select_only()
        .column_as(
            Expr::cust("COALESCE(SUM(CAST(duration_secs AS DOUBLE PRECISION)), 0)"),
            "total",
        )
        .into_model::<SumResult>()
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_duration_secs: f64 = duration_result.and_then(|r| r.total).unwrap_or(0.0);

    // Get P2P peer count
    let total_peers = if let Some(ref p2p) = state.p2p {
        if let Ok(node) = p2p.clone().downcast::<soundtime_p2p::P2pNode>() {
            node.registry().peer_count().await as u64
        } else {
            0
        }
    } else {
        0
    };

    Ok(Json(StatsOverview {
        total_tracks,
        total_albums,
        total_artists,
        total_genres,
        total_duration_secs,
        total_peers,
    }))
}
