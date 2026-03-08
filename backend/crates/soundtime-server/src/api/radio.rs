//! Radio feature — generates continuous track recommendations.
//!
//! A single stateless endpoint (`POST /api/radio/next`) returns a batch of
//! tracks based on a seed (track, artist, genre, similar, or personal mix).
//! The frontend sends previously played track IDs to avoid duplicates.
//!
//! Each seed algorithm uses a multi-phase approach with **quota enforcement**:
//! phases fetch a pool of candidates (`count * 2`) but are then truncated to
//! their target quota before merging, so the final mix respects the intended
//! ratio (e.g. 50/30/20). The merged results are deduplicated, shuffled, and
//! trimmed to the requested `count`.
//!
//! The personal-mix algorithm uses **completion-ratio weighting** — tracks
//! that were listened to more completely receive higher weight than skips,
//! so the radio favours music the user genuinely enjoys.
//!
//! The similar algorithm uses **pgvector cosine distance** over 32-dimensional
//! track embeddings to find tracks with similar audio/metadata characteristics.
//! It falls back to the track-based algorithm when no embedding is available.

use axum::{extract::State, http::StatusCode, Json};
use sea_orm::{
    sea_query::Expr, ColumnTrait, EntityTrait, Order, QueryFilter, QueryOrder, QuerySelect,
};
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
    /// Seed from a specific track — same artist, genre, and era.
    Track,
    /// Seed from an artist — their tracks plus same-genre tracks by others.
    Artist,
    /// Seed from a genre string — random tracks matching that genre.
    Genre,
    /// Personal mix — weighted blend of the user's listening history and favorites.
    PersonalMix,
    /// Embedding-based similarity — uses pgvector cosine distance to find
    /// tracks with similar metadata characteristics. Falls back to `Track`
    /// when the seed has no embedding.
    Similar,
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
/// Builds a mix from three phases, each fetching up to `count * 2` candidates
/// then truncating to its quota before merging:
///
/// - **Phase 1** — Same artist (quota: 50% of `count`)
/// - **Phase 2** — Same genre, different artist (quota: 30% of `count`)
/// - **Phase 3** — Same era ±5 years (quota: remaining, up to `count`)
///
/// After merging, results are deduplicated by track ID, shuffled, and trimmed
/// to `count`. The quota enforcement ensures each phase contributes its
/// intended proportion even when one phase has abundant candidates.
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

    let pool_size = count * 2;
    let exclude_vec: Vec<Uuid> = exclude.iter().copied().collect();

    // Phase quotas to enforce the 50/30/20 ratio
    let phase1_quota = (count as f64 * 0.5).ceil() as usize;
    let phase2_quota = (count as f64 * 0.3).ceil() as usize;
    let phase3_quota = count as usize; // remainder, trimmed by final take

    // Phase 1 — Same artist (50%)
    let mut phase1_query = track::Entity::find().filter(track::Column::ArtistId.eq(seed.artist_id));
    if !exclude_vec.is_empty() {
        phase1_query = phase1_query.filter(track::Column::Id.is_not_in(exclude_vec.clone()));
    }
    let phase1: Vec<track::Model> = phase1_query
        .order_by(Expr::cust("RANDOM()"), Order::Asc)
        .limit(pool_size)
        .all(db)
        .await
        .unwrap_or_default();
    let phase1: Vec<track::Model> = phase1.into_iter().take(phase1_quota).collect();

    // Phase 2 — Same genre, different artist (30%)
    let mut phase2 = Vec::new();
    if let Some(ref genre) = seed.genre {
        let mut q = track::Entity::find()
            .filter(track::Column::Genre.eq(genre.clone()))
            .filter(track::Column::ArtistId.ne(seed.artist_id));
        if !exclude_vec.is_empty() {
            q = q.filter(track::Column::Id.is_not_in(exclude_vec.clone()));
        }
        phase2 = q
            .order_by(Expr::cust("RANDOM()"), Order::Asc)
            .limit(pool_size)
            .all(db)
            .await
            .unwrap_or_default();
    }
    let phase2: Vec<track::Model> = phase2.into_iter().take(phase2_quota).collect();

    // Phase 3 — Same era ±5 years (20%)
    let mut phase3 = Vec::new();
    if let Some(year) = seed.year {
        let mut q = track::Entity::find()
            .filter(track::Column::Year.gte(year - 5))
            .filter(track::Column::Year.lte(year + 5));
        if !exclude_vec.is_empty() {
            q = q.filter(track::Column::Id.is_not_in(exclude_vec));
        }
        phase3 = q
            .order_by(Expr::cust("RANDOM()"), Order::Asc)
            .limit(pool_size)
            .all(db)
            .await
            .unwrap_or_default();
    }
    let phase3: Vec<track::Model> = phase3.into_iter().take(phase3_quota).collect();

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
/// Builds a mix from two phases, each fetching up to `count * 2` candidates
/// then truncating to its quota before merging:
///
/// - **Phase 1** — Tracks by this artist (quota: 60% of `count`)
/// - **Phase 2** — Same genre(s) by other artists (quota: 40% of `count`)
///
/// The artist's existing tracks are scanned to determine their genres, which
/// are then used to find similar music from other artists in Phase 2.
async fn seed_artist(
    db: &sea_orm::DatabaseConnection,
    seed_id: Uuid,
    count: u64,
    exclude: &HashSet<Uuid>,
) -> Result<Vec<track::Model>, (StatusCode, String)> {
    let pool_size = count * 2;
    let exclude_vec: Vec<Uuid> = exclude.iter().copied().collect();

    // Phase quotas to enforce the 60/40 ratio
    let phase1_quota = (count as f64 * 0.6).ceil() as usize;
    let phase2_quota = (count as f64 * 0.4).ceil() as usize;

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

    // Phase 1 — Artist's own tracks (60%)
    let mut phase1_query = track::Entity::find().filter(track::Column::ArtistId.eq(seed_id));
    if !exclude_vec.is_empty() {
        phase1_query = phase1_query.filter(track::Column::Id.is_not_in(exclude_vec.clone()));
    }
    let phase1: Vec<track::Model> = phase1_query
        .order_by(Expr::cust("RANDOM()"), Order::Asc)
        .limit(pool_size)
        .all(db)
        .await
        .unwrap_or_default();
    let phase1: Vec<track::Model> = phase1.into_iter().take(phase1_quota).collect();

    // Phase 2 — Same genre, other artists (40%)
    let mut phase2 = Vec::new();
    if !genres.is_empty() {
        let mut q = track::Entity::find()
            .filter(track::Column::Genre.is_in(genres))
            .filter(track::Column::ArtistId.ne(seed_id));
        if !exclude_vec.is_empty() {
            q = q.filter(track::Column::Id.is_not_in(exclude_vec));
        }
        phase2 = q
            .order_by(Expr::cust("RANDOM()"), Order::Asc)
            .limit(pool_size)
            .all(db)
            .await
            .unwrap_or_default();
    }
    let phase2: Vec<track::Model> = phase2.into_iter().take(phase2_quota).collect();

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
/// Single-phase: fetches up to `count * 3` (capped at 200) tracks matching
/// the given genre, shuffles them, and returns `count`. No quota enforcement
/// needed since there is only one phase.
async fn seed_genre(
    db: &sea_orm::DatabaseConnection,
    genre: &str,
    count: u64,
    exclude: &HashSet<Uuid>,
) -> Result<Vec<track::Model>, (StatusCode, String)> {
    let pool_size = (count * 3).min(200);
    let exclude_vec: Vec<Uuid> = exclude.iter().copied().collect();

    let mut query = track::Entity::find().filter(track::Column::Genre.eq(genre));
    if !exclude_vec.is_empty() {
        query = query.filter(track::Column::Id.is_not_in(exclude_vec));
    }

    let mut pool = query
        .order_by(Expr::cust("RANDOM()"), Order::Asc)
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
/// Draws from listen history and favorites across three phases, each
/// fetching up to `count * 2` candidates then truncating to its quota:
///
/// - **Phase 1** — Tracks from top 10 weighted artists (quota: 40%)
/// - **Phase 2** — Tracks from top 5 weighted genres, excluding Phase 1
///   artists (quota: 40%)
/// - **Phase 3** — Random tracks for discovery (quota: 20%)
///
/// ## Completion-ratio weighting
///
/// Instead of counting raw listens, each listen is weighted by how much
/// of the track was actually heard:
///
/// - **Full/partial listens** (≥25% completion): weight = `min(duration_listened / duration_secs, 1.0)`
/// - **Skips** (<25% completion): weight = `0.1` (still registers engagement)
/// - **Favorites**: weight = `1.0` each (additive with listen weights)
///
/// These per-track weights are aggregated by artist and genre to determine
/// the top artists and genres. This ensures the radio prioritises music
/// the user genuinely enjoys over tracks that were merely started and skipped.
///
/// Falls back to fully random tracks when the user has no listening history
/// or favorites.
async fn seed_personal_mix(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
    count: u64,
    exclude: &HashSet<Uuid>,
) -> Result<Vec<track::Model>, (StatusCode, String)> {
    let pool_size = count * 2;
    let exclude_vec: Vec<Uuid> = exclude.iter().copied().collect();

    // Phase quotas to enforce the 40/40/20 ratio
    let phase1_quota = (count as f64 * 0.4).ceil() as usize;
    let phase2_quota = (count as f64 * 0.4).ceil() as usize;
    let phase3_quota = (count as f64 * 0.2).ceil() as usize;

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
            .order_by(Expr::cust("RANDOM()"), Order::Asc)
            .limit((count * 3).min(200))
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

    // Build a map of track_id -> track for lookups
    let track_map: HashMap<Uuid, &track::Model> =
        referenced_tracks.iter().map(|t| (t.id, t)).collect();

    // Accumulate listen weights per track from history.
    // Weight = min(duration_listened / max(duration_secs, 1.0), 1.0) to cap at 1.0.
    // Skips (<25% listened) count as 0.1 to still register engagement.
    let mut track_weights: HashMap<Uuid, f64> = HashMap::new();
    for listen in &recent_listens {
        if let Some(t) = track_map.get(&listen.track_id) {
            let duration = t.duration_secs as f64;
            let completion = if duration > 0.0 {
                (listen.duration_listened as f64 / duration).min(1.0)
            } else {
                1.0
            };
            let weight = if completion < 0.25 { 0.1 } else { completion };
            *track_weights.entry(listen.track_id).or_insert(0.0) += weight;
        }
    }

    // Also give weight to favorites (count as 1.0 each)
    for fav_id in &fav_track_ids {
        *track_weights.entry(*fav_id).or_insert(0.0) += 1.0;
    }

    // Aggregate weights by artist and genre
    let mut artist_freq: HashMap<Uuid, f64> = HashMap::new();
    let mut genre_freq: HashMap<String, f64> = HashMap::new();
    for (track_id, weight) in &track_weights {
        if let Some(t) = track_map.get(track_id) {
            *artist_freq.entry(t.artist_id).or_insert(0.0) += weight;
            if let Some(ref g) = t.genre {
                *genre_freq.entry(g.clone()).or_insert(0.0) += weight;
            }
        }
    }

    // Sort by weighted frequency (highest first)
    let mut top_artists: Vec<Uuid> = artist_freq.keys().copied().collect();
    top_artists.sort_by(|a, b| {
        artist_freq[b]
            .partial_cmp(&artist_freq[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let top_artists: Vec<Uuid> = top_artists.into_iter().take(10).collect();

    let mut top_genres: Vec<String> = genre_freq.keys().cloned().collect();
    top_genres.sort_by(|a, b| {
        genre_freq[b]
            .partial_cmp(&genre_freq[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let top_genres: Vec<String> = top_genres.into_iter().take(5).collect();

    // Phase 1 — Tracks from favorite artists (40%)
    let mut phase1 = Vec::new();
    if !top_artists.is_empty() {
        let mut q =
            track::Entity::find().filter(track::Column::ArtistId.is_in(top_artists.clone()));
        if !exclude_vec.is_empty() {
            q = q.filter(track::Column::Id.is_not_in(exclude_vec.clone()));
        }
        phase1 = q
            .order_by(Expr::cust("RANDOM()"), Order::Asc)
            .limit(pool_size)
            .all(db)
            .await
            .unwrap_or_default();
    }
    let phase1: Vec<track::Model> = phase1.into_iter().take(phase1_quota).collect();

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
        phase2 = q
            .order_by(Expr::cust("RANDOM()"), Order::Asc)
            .limit(pool_size)
            .all(db)
            .await
            .unwrap_or_default();
    }
    let phase2: Vec<track::Model> = phase2.into_iter().take(phase2_quota).collect();

    // Phase 3 — Random tracks for discovery (20%)
    let mut phase3_query = track::Entity::find();
    if !exclude_vec.is_empty() {
        phase3_query = phase3_query.filter(track::Column::Id.is_not_in(exclude_vec));
    }
    let phase3: Vec<track::Model> = phase3_query
        .order_by(Expr::cust("RANDOM()"), Order::Asc)
        .limit(pool_size)
        .all(db)
        .await
        .unwrap_or_default();
    let phase3: Vec<track::Model> = phase3.into_iter().take(phase3_quota).collect();

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

/// Seed algorithm: similarity-based radio using vector embeddings.
///
/// Uses pgvector cosine distance to find tracks with similar audio
/// characteristics to the seed track. Falls back to `seed_track()` if
/// the seed track has no embedding (e.g. during backfill).
///
/// The embedding captures genre, era, duration, audio quality, popularity,
/// and artist characteristics in a 32-dimensional vector, so "similar"
/// tracks share multiple metadata attributes — not just one dimension.
async fn seed_similar(
    db: &sea_orm::DatabaseConnection,
    seed_id: Uuid,
    count: u64,
    exclude: &HashSet<Uuid>,
) -> Result<Vec<track::Model>, (StatusCode, String)> {
    // Get or generate the seed track's embedding
    let embedding = match crate::embeddings::get_track_embedding(db, seed_id).await {
        Ok(Some(emb)) => emb,
        Ok(None) => {
            // No embedding yet — try to generate one on the fly
            let seed = track::Entity::find_by_id(seed_id)
                .one(db)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
                .ok_or((StatusCode::NOT_FOUND, "Seed track not found".to_string()))?;

            if let Err(e) = crate::embeddings::generate_and_store_embedding(db, &seed).await {
                tracing::warn!(error = %e, track_id = %seed_id, "failed to generate embedding on the fly, falling back to seed_track");
                return seed_track(db, seed_id, count, exclude).await;
            }

            match crate::embeddings::get_track_embedding(db, seed_id).await {
                Ok(Some(emb)) => emb,
                _ => return seed_track(db, seed_id, count, exclude).await,
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, track_id = %seed_id, "failed to fetch embedding, falling back to seed_track");
            return seed_track(db, seed_id, count, exclude).await;
        }
    };

    // Query pgvector for similar tracks (fetch extra to account for excludes)
    let exclude_vec: Vec<Uuid> = exclude.iter().copied().collect();
    let similar_ids = crate::embeddings::find_similar_tracks(
        db,
        &embedding,
        count * 3, // over-fetch to have room after filtering
        &exclude_vec,
    )
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "pgvector similarity search failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Similarity search failed: {e}"),
        )
    })?;

    if similar_ids.is_empty() {
        tracing::info!(track_id = %seed_id, "no similar tracks found via embeddings, falling back to seed_track");
        return seed_track(db, seed_id, count, exclude).await;
    }

    // Fetch full track models for the returned IDs, preserving similarity order
    let ids: Vec<Uuid> = similar_ids.iter().map(|(id, _)| *id).collect();
    let tracks = track::Entity::find()
        .filter(track::Column::Id.is_in(ids.clone()))
        .all(db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    // Re-order tracks by similarity (closest first), then shuffle slightly
    let track_map: HashMap<Uuid, track::Model> = tracks.into_iter().map(|t| (t.id, t)).collect();
    let mut ordered: Vec<track::Model> = ids
        .into_iter()
        .filter_map(|id| track_map.get(&id).cloned())
        .collect();

    // Light shuffle — swap adjacent pairs with 30% probability for variety
    use rand::Rng;
    let mut rng = rand::rng();
    let len = ordered.len();
    for i in (0..len.saturating_sub(1)).step_by(2) {
        if rng.random::<f32>() < 0.3 {
            ordered.swap(i, i + 1);
        }
    }

    Ok(ordered.into_iter().take(count as usize).collect())
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
            let result = seed_track(&state.db, seed_id, count, &exclude).await?;
            if result.is_empty() {
                tracing::info!(
                    seed_type = %seed_type_debug,
                    "radio wrapped around — clearing exclude list and retrying"
                );
                seed_track(&state.db, seed_id, count, &HashSet::new()).await?
            } else {
                result
            }
        }
        RadioSeedType::Artist => {
            let seed_id = body.seed_id.ok_or((
                StatusCode::BAD_REQUEST,
                "seed_id is required for seed_type=artist".to_string(),
            ))?;
            let result = seed_artist(&state.db, seed_id, count, &exclude).await?;
            if result.is_empty() {
                tracing::info!(
                    seed_type = %seed_type_debug,
                    "radio wrapped around — clearing exclude list and retrying"
                );
                seed_artist(&state.db, seed_id, count, &HashSet::new()).await?
            } else {
                result
            }
        }
        RadioSeedType::Genre => {
            let genre = body.genre.ok_or((
                StatusCode::BAD_REQUEST,
                "genre is required for seed_type=genre".to_string(),
            ))?;
            let result = seed_genre(&state.db, &genre, count, &exclude).await?;
            if result.is_empty() {
                tracing::info!(
                    seed_type = %seed_type_debug,
                    "radio wrapped around — clearing exclude list and retrying"
                );
                seed_genre(&state.db, &genre, count, &HashSet::new()).await?
            } else {
                result
            }
        }
        RadioSeedType::PersonalMix => {
            let result = seed_personal_mix(&state.db, auth_user.0.sub, count, &exclude).await?;
            if result.is_empty() {
                tracing::info!(
                    seed_type = %seed_type_debug,
                    "radio wrapped around — clearing exclude list and retrying"
                );
                seed_personal_mix(&state.db, auth_user.0.sub, count, &HashSet::new()).await?
            } else {
                result
            }
        }
        RadioSeedType::Similar => {
            let seed_id = body.seed_id.ok_or((
                StatusCode::BAD_REQUEST,
                "seed_id is required for seed_type=similar".to_string(),
            ))?;
            let result = seed_similar(&state.db, seed_id, count, &exclude).await?;
            if result.is_empty() {
                tracing::info!(
                    seed_type = %seed_type_debug,
                    "radio wrapped around — clearing exclude list and retrying"
                );
                seed_similar(&state.db, seed_id, count, &HashSet::new()).await?
            } else {
                result
            }
        }
    };

    let exhausted = selected.is_empty();
    if exhausted {
        tracing::info!(
            seed_type = %seed_type_debug,
            "radio exhausted — no matching tracks in database"
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
    fn test_deserialize_radio_next_request_similar() {
        let json = r#"{"seed_type":"similar","seed_id":"550e8400-e29b-41d4-a716-446655440000","count":10}"#;
        let req: RadioNextRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req.seed_type, RadioSeedType::Similar));
        assert!(req.seed_id.is_some());
        assert_eq!(req.count, Some(10));
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
            (r#""similar""#, "Similar"),
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
