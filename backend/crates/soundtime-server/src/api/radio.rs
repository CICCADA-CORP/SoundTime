//! Radio feature — generates continuous track recommendations.
//!
//! A single stateless endpoint (`POST /api/radio/next`) returns a batch of
//! tracks based on a seed (track, artist, genre, or personal mix). The
//! frontend sends previously played track IDs to avoid duplicates.

use axum::{extract::State, http::StatusCode, Json};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use soundtime_db::entities::{album, artist, favorite, listen_history, track};
use soundtime_db::AppState;

// ─── Structs ────────────────────────────────────────────────────────────

/// Types of seeds for the radio.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum RadioSeedType {
    Track,
    Artist,
    Genre,
    PersonalMix,
}

/// Request body for `POST /api/radio/next`.
#[derive(Debug, Deserialize)]
pub struct RadioNextRequest {
    /// Type of seed
    pub seed_type: RadioSeedType,
    /// ID of the seed track or artist (required for seed_type = track | artist)
    pub seed_id: Option<Uuid>,
    /// Genre string (required for seed_type = genre)
    pub genre: Option<String>,
    /// Number of tracks to return (default: 5, max: 20)
    pub count: Option<u64>,
    /// IDs of tracks already played in this session (to exclude)
    #[serde(default)]
    pub exclude: Vec<Uuid>,
}

/// Response body — reuses `TrackResponse` from tracks module.
#[derive(Debug, Serialize)]
pub struct RadioNextResponse {
    pub tracks: Vec<super::tracks::TrackResponse>,
    /// Indicates whether the radio has exhausted available tracks
    pub exhausted: bool,
}

// ─── Helper: enrich tracks with artist/album data ───────────────────────

/// Batch-fetch artist and album data for a set of tracks and build
/// enriched `TrackResponse` objects. Reuses the same pattern as
/// `list_tracks` and `list_random_tracks` in `tracks.rs`.
async fn enrich_tracks(
    db: &sea_orm::DatabaseConnection,
    tracks: Vec<track::Model>,
) -> Result<Vec<super::tracks::TrackResponse>, (StatusCode, String)> {
    let artist_ids: Vec<Uuid> = tracks
        .iter()
        .map(|t| t.artist_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let album_ids: Vec<Uuid> = tracks
        .iter()
        .filter_map(|t| t.album_id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let artists: HashMap<Uuid, artist::Model> = if !artist_ids.is_empty() {
        artist::Entity::find()
            .filter(artist::Column::Id.is_in(artist_ids))
            .all(db)
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
            .all(db)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a))
            .collect()
    } else {
        HashMap::new()
    };

    let data = tracks
        .into_iter()
        .map(|t| {
            let artist_name = artists.get(&t.artist_id).map(|a| a.name.clone());
            let (album_title, cover_url) = t
                .album_id
                .and_then(|aid| albums.get(&aid))
                .map(|a| {
                    (
                        Some(a.title.clone()),
                        a.cover_url.clone().map(|url| {
                            if url.starts_with("/api/media/") || url.starts_with("http") {
                                url
                            } else {
                                format!("/api/media/{url}")
                            }
                        }),
                    )
                })
                .unwrap_or((None, None));

            let mut resp = super::tracks::TrackResponse::from(t);
            resp.artist_name = artist_name;
            resp.album_title = album_title;
            resp.cover_url = cover_url;
            resp
        })
        .collect();

    Ok(data)
}

// ─── Seed algorithms ────────────────────────────────────────────────────

/// Seed algorithm: track-based radio.
///
/// Phase 1 — Same artist (50%), Phase 2 — Same genre (30%),
/// Phase 3 — Same era ±5 years (20%), merged and shuffled.
async fn seed_track(
    db: &sea_orm::DatabaseConnection,
    seed_id: Uuid,
    count: u64,
    exclude: &HashSet<Uuid>,
) -> Result<Vec<track::Model>, (StatusCode, String)> {
    let seed = track::Entity::find_by_id(seed_id)
        .one(db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Seed track not found".to_string()))?;

    let pool_size = count * 3;
    let exclude_vec: Vec<Uuid> = exclude.iter().copied().collect();

    // Phase 1 — Same artist
    let mut phase1_query = track::Entity::find().filter(track::Column::ArtistId.eq(seed.artist_id));
    if !exclude_vec.is_empty() {
        phase1_query = phase1_query.filter(track::Column::Id.is_not_in(exclude_vec.clone()));
    }
    let phase1 = phase1_query
        .limit(pool_size)
        .all(db)
        .await
        .unwrap_or_default();

    // Phase 2 — Same genre, different artist
    let mut phase2 = Vec::new();
    if let Some(ref genre) = seed.genre {
        let mut q = track::Entity::find()
            .filter(track::Column::Genre.eq(genre.clone()))
            .filter(track::Column::ArtistId.ne(seed.artist_id));
        if !exclude_vec.is_empty() {
            q = q.filter(track::Column::Id.is_not_in(exclude_vec.clone()));
        }
        phase2 = q.limit(pool_size).all(db).await.unwrap_or_default();
    }

    // Phase 3 — Same era ±5 years
    let mut phase3 = Vec::new();
    if let Some(year) = seed.year {
        let mut q = track::Entity::find()
            .filter(track::Column::Year.gte(year - 5))
            .filter(track::Column::Year.lte(year + 5));
        if !exclude_vec.is_empty() {
            q = q.filter(track::Column::Id.is_not_in(exclude_vec));
        }
        phase3 = q.limit(pool_size).all(db).await.unwrap_or_default();
    }

    // Merge, deduplicate, shuffle, take count
    let mut pool: Vec<track::Model> = Vec::new();
    let mut seen: HashSet<Uuid> = HashSet::new();
    for t in phase1.into_iter().chain(phase2).chain(phase3) {
        if seen.insert(t.id) {
            pool.push(t);
        }
    }

    use rand::seq::SliceRandom;
    let mut rng = rand::rng();
    pool.shuffle(&mut rng);

    Ok(pool.into_iter().take(count as usize).collect())
}

/// Seed algorithm: artist-based radio.
///
/// Phase 1 — Tracks by this artist (60%), Phase 2 — Same genre(s) by
/// other artists (40%).
async fn seed_artist(
    db: &sea_orm::DatabaseConnection,
    seed_id: Uuid,
    count: u64,
    exclude: &HashSet<Uuid>,
) -> Result<Vec<track::Model>, (StatusCode, String)> {
    let pool_size = count * 3;
    let exclude_vec: Vec<Uuid> = exclude.iter().copied().collect();

    // Get artist's tracks to determine genres
    let artist_tracks = track::Entity::find()
        .filter(track::Column::ArtistId.eq(seed_id))
        .all(db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    if artist_tracks.is_empty() {
        // Check if artist exists
        let artist_exists = artist::Entity::find_by_id(seed_id)
            .one(db)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
        if artist_exists.is_none() {
            return Err((StatusCode::NOT_FOUND, "Seed artist not found".to_string()));
        }
    }

    let genres: Vec<String> = artist_tracks
        .iter()
        .filter_map(|t| t.genre.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    // Phase 1 — Artist's own tracks
    let mut phase1_query = track::Entity::find().filter(track::Column::ArtistId.eq(seed_id));
    if !exclude_vec.is_empty() {
        phase1_query = phase1_query.filter(track::Column::Id.is_not_in(exclude_vec.clone()));
    }
    let phase1 = phase1_query
        .limit(pool_size)
        .all(db)
        .await
        .unwrap_or_default();

    // Phase 2 — Same genre, other artists
    let mut phase2 = Vec::new();
    if !genres.is_empty() {
        let mut q = track::Entity::find()
            .filter(track::Column::Genre.is_in(genres))
            .filter(track::Column::ArtistId.ne(seed_id));
        if !exclude_vec.is_empty() {
            q = q.filter(track::Column::Id.is_not_in(exclude_vec));
        }
        phase2 = q.limit(pool_size).all(db).await.unwrap_or_default();
    }

    // Merge, deduplicate, shuffle, take count
    let mut pool: Vec<track::Model> = Vec::new();
    let mut seen: HashSet<Uuid> = HashSet::new();
    for t in phase1.into_iter().chain(phase2) {
        if seen.insert(t.id) {
            pool.push(t);
        }
    }

    use rand::seq::SliceRandom;
    let mut rng = rand::rng();
    pool.shuffle(&mut rng);

    Ok(pool.into_iter().take(count as usize).collect())
}

/// Seed algorithm: genre-based radio.
///
/// Simple genre filter with shuffle.
async fn seed_genre(
    db: &sea_orm::DatabaseConnection,
    genre: &str,
    count: u64,
    exclude: &HashSet<Uuid>,
) -> Result<Vec<track::Model>, (StatusCode, String)> {
    let pool_size = (count * 5).min(500);
    let exclude_vec: Vec<Uuid> = exclude.iter().copied().collect();

    let mut query = track::Entity::find().filter(track::Column::Genre.eq(genre));
    if !exclude_vec.is_empty() {
        query = query.filter(track::Column::Id.is_not_in(exclude_vec));
    }

    let mut pool = query
        .limit(pool_size)
        .all(db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    use rand::seq::SliceRandom;
    let mut rng = rand::rng();
    pool.shuffle(&mut rng);

    Ok(pool.into_iter().take(count as usize).collect())
}

/// Seed algorithm: personal mix.
///
/// Draws from listen history and favorites: 40% favorite artists, 40%
/// favorite genres (different artists), 20% random (discovery).
async fn seed_personal_mix(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
    count: u64,
    exclude: &HashSet<Uuid>,
) -> Result<Vec<track::Model>, (StatusCode, String)> {
    let pool_size = count * 3;
    let exclude_vec: Vec<Uuid> = exclude.iter().copied().collect();

    // Get the 50 most recent listens
    let recent_listens = listen_history::Entity::find()
        .filter(listen_history::Column::UserId.eq(user_id))
        .order_by_desc(listen_history::Column::ListenedAt)
        .limit(50)
        .all(db)
        .await
        .unwrap_or_default();

    // Get favorites
    let favorites = favorite::Entity::find()
        .filter(favorite::Column::UserId.eq(user_id))
        .all(db)
        .await
        .unwrap_or_default();

    // Collect track IDs from history + favorites
    let history_track_ids: Vec<Uuid> = recent_listens.iter().map(|h| h.track_id).collect();
    let fav_track_ids: Vec<Uuid> = favorites.iter().map(|f| f.track_id).collect();

    let all_track_ids: Vec<Uuid> = history_track_ids
        .iter()
        .chain(fav_track_ids.iter())
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if all_track_ids.is_empty() {
        // Fallback: random tracks
        let mut pool = track::Entity::find()
            .limit((count * 5).min(500))
            .all(db)
            .await
            .unwrap_or_default();

        if !exclude_vec.is_empty() {
            let exclude_set: HashSet<Uuid> = exclude_vec.into_iter().collect();
            pool.retain(|t| !exclude_set.contains(&t.id));
        }

        use rand::seq::SliceRandom;
        let mut rng = rand::rng();
        pool.shuffle(&mut rng);

        return Ok(pool.into_iter().take(count as usize).collect());
    }

    // Fetch the referenced tracks to extract artists and genres
    let referenced_tracks = track::Entity::find()
        .filter(track::Column::Id.is_in(all_track_ids))
        .all(db)
        .await
        .unwrap_or_default();

    // Count artist and genre frequency
    let mut artist_freq: HashMap<Uuid, usize> = HashMap::new();
    let mut genre_freq: HashMap<String, usize> = HashMap::new();
    for t in &referenced_tracks {
        *artist_freq.entry(t.artist_id).or_insert(0) += 1;
        if let Some(ref g) = t.genre {
            *genre_freq.entry(g.clone()).or_insert(0) += 1;
        }
    }

    // Sort by frequency (most frequent first)
    let mut top_artists: Vec<Uuid> = artist_freq.keys().copied().collect();
    top_artists.sort_by(|a, b| artist_freq[b].cmp(&artist_freq[a]));
    let top_artists: Vec<Uuid> = top_artists.into_iter().take(10).collect();

    let mut top_genres: Vec<String> = genre_freq.keys().cloned().collect();
    top_genres.sort_by(|a, b| genre_freq[b].cmp(&genre_freq[a]));
    let top_genres: Vec<String> = top_genres.into_iter().take(5).collect();

    // Phase 1 — Tracks from favorite artists (40%)
    let mut phase1 = Vec::new();
    if !top_artists.is_empty() {
        let mut q =
            track::Entity::find().filter(track::Column::ArtistId.is_in(top_artists.clone()));
        if !exclude_vec.is_empty() {
            q = q.filter(track::Column::Id.is_not_in(exclude_vec.clone()));
        }
        phase1 = q.limit(pool_size).all(db).await.unwrap_or_default();
    }

    // Phase 2 — Tracks from favorite genres, different artists (40%)
    let mut phase2 = Vec::new();
    if !top_genres.is_empty() {
        let mut q = track::Entity::find().filter(track::Column::Genre.is_in(top_genres));
        if !top_artists.is_empty() {
            q = q.filter(track::Column::ArtistId.is_not_in(top_artists));
        }
        if !exclude_vec.is_empty() {
            q = q.filter(track::Column::Id.is_not_in(exclude_vec.clone()));
        }
        phase2 = q.limit(pool_size).all(db).await.unwrap_or_default();
    }

    // Phase 3 — Random tracks for discovery (20%)
    let mut phase3_query = track::Entity::find();
    if !exclude_vec.is_empty() {
        phase3_query = phase3_query.filter(track::Column::Id.is_not_in(exclude_vec));
    }
    let phase3 = phase3_query
        .limit(pool_size)
        .all(db)
        .await
        .unwrap_or_default();

    // Merge, deduplicate, shuffle, take count
    let mut pool: Vec<track::Model> = Vec::new();
    let mut seen: HashSet<Uuid> = HashSet::new();
    for t in phase1.into_iter().chain(phase2).chain(phase3) {
        if seen.insert(t.id) {
            pool.push(t);
        }
    }

    use rand::seq::SliceRandom;
    let mut rng = rand::rng();
    pool.shuffle(&mut rng);

    Ok(pool.into_iter().take(count as usize).collect())
}

// ─── Handler ────────────────────────────────────────────────────────────

/// POST /api/radio/next (auth required)
///
/// Returns the next batch of radio tracks based on the given seed.
pub async fn radio_next(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Json(body): Json<RadioNextRequest>,
) -> Result<Json<RadioNextResponse>, (StatusCode, String)> {
    let count = body.count.unwrap_or(5).min(20);

    // Limit exclude list to 2000 IDs
    let exclude: HashSet<Uuid> = if body.exclude.len() > 2000 {
        tracing::warn!(
            user_id = %auth_user.0.sub,
            exclude_count = body.exclude.len(),
            "Radio exclude list exceeds 2000, truncating"
        );
        body.exclude.into_iter().take(2000).collect()
    } else {
        body.exclude.into_iter().collect()
    };

    let seed_type_debug = format!("{:?}", body.seed_type);

    let selected = match body.seed_type {
        RadioSeedType::Track => {
            let seed_id = body.seed_id.ok_or((
                StatusCode::BAD_REQUEST,
                "seed_id is required for seed_type=track".to_string(),
            ))?;
            seed_track(&state.db, seed_id, count, &exclude).await?
        }
        RadioSeedType::Artist => {
            let seed_id = body.seed_id.ok_or((
                StatusCode::BAD_REQUEST,
                "seed_id is required for seed_type=artist".to_string(),
            ))?;
            seed_artist(&state.db, seed_id, count, &exclude).await?
        }
        RadioSeedType::Genre => {
            let genre = body.genre.ok_or((
                StatusCode::BAD_REQUEST,
                "genre is required for seed_type=genre".to_string(),
            ))?;
            seed_genre(&state.db, &genre, count, &exclude).await?
        }
        RadioSeedType::PersonalMix => {
            seed_personal_mix(&state.db, auth_user.0.sub, count, &exclude).await?
        }
    };

    let exhausted = selected.is_empty();
    if exhausted {
        tracing::info!(
            seed_type = %seed_type_debug,
            "radio exhausted — all tracks excluded"
        );
    }

    let tracks = enrich_tracks(&state.db, selected).await?;

    Ok(Json(RadioNextResponse { tracks, exhausted }))
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_radio_next_request_track() {
        let json = r#"{"seed_type":"track","seed_id":"550e8400-e29b-41d4-a716-446655440000","count":5,"exclude":[]}"#;
        let req: RadioNextRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req.seed_type, RadioSeedType::Track));
        assert!(req.seed_id.is_some());
        assert_eq!(req.count, Some(5));
        assert!(req.exclude.is_empty());
    }

    #[test]
    fn test_deserialize_radio_next_request_artist() {
        let json = r#"{"seed_type":"artist","seed_id":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let req: RadioNextRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req.seed_type, RadioSeedType::Artist));
        assert!(req.seed_id.is_some());
        assert!(req.count.is_none());
        assert!(req.exclude.is_empty());
    }

    #[test]
    fn test_deserialize_radio_next_request_genre() {
        let json = r#"{"seed_type":"genre","genre":"Rock","count":10}"#;
        let req: RadioNextRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req.seed_type, RadioSeedType::Genre));
        assert_eq!(req.genre, Some("Rock".to_string()));
        assert!(req.seed_id.is_none());
    }

    #[test]
    fn test_deserialize_radio_next_request_personal_mix() {
        let json = r#"{"seed_type":"personal_mix"}"#;
        let req: RadioNextRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req.seed_type, RadioSeedType::PersonalMix));
        assert!(req.seed_id.is_none());
        assert!(req.genre.is_none());
    }

    #[test]
    fn test_radio_next_request_defaults() {
        let json = r#"{"seed_type":"genre","genre":"Pop"}"#;
        let req: RadioNextRequest = serde_json::from_str(json).unwrap();
        assert!(req.count.is_none());
        assert!(req.exclude.is_empty());
    }

    #[test]
    fn test_radio_seed_type_variants() {
        let variants = [
            (r#""track""#, "Track"),
            (r#""artist""#, "Artist"),
            (r#""genre""#, "Genre"),
            (r#""personal_mix""#, "PersonalMix"),
        ];
        for (json, expected_debug) in variants {
            let st: RadioSeedType = serde_json::from_str(json).unwrap();
            assert!(format!("{:?}", st).contains(expected_debug));
        }
    }

    #[test]
    fn test_deserialize_with_exclude_list() {
        let json = r#"{
            "seed_type": "track",
            "seed_id": "550e8400-e29b-41d4-a716-446655440000",
            "exclude": [
                "550e8400-e29b-41d4-a716-446655440001",
                "550e8400-e29b-41d4-a716-446655440002"
            ]
        }"#;
        let req: RadioNextRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.exclude.len(), 2);
    }

    #[test]
    fn test_serialize_radio_next_response() {
        let resp = RadioNextResponse {
            tracks: vec![],
            exhausted: true,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["exhausted"], true);
        assert!(json["tracks"].as_array().unwrap().is_empty());
    }
}
