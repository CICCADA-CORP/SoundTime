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

    // Count distinct genres
    use sea_orm::{FromQueryResult, QuerySelect};

    #[derive(Debug, FromQueryResult)]
    struct GenreRow {
        genre: Option<String>,
    }

    let genre_rows = track::Entity::find()
        .select_only()
        .column(track::Column::Genre)
        .distinct()
        .into_model::<GenreRow>()
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_genres = genre_rows
        .into_iter()
        .filter(|r| r.genre.as_ref().is_some_and(|g| !g.is_empty()))
        .count() as u64;

    // Sum all durations
    #[derive(Debug, FromQueryResult)]
    struct DurationRow {
        duration_secs: f32,
    }

    let duration_rows = track::Entity::find()
        .select_only()
        .column(track::Column::DurationSecs)
        .into_model::<DurationRow>()
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let total_duration_secs: f64 = duration_rows.iter().map(|r| r.duration_secs as f64).sum();

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
