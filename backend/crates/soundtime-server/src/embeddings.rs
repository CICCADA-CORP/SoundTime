//! Track embedding generation and vector similarity operations.
//!
//! Generates handcrafted 32-dimensional feature vectors from track metadata.
//! No ML dependencies — features are deterministically computed from genre,
//! year, duration, audio quality, and popularity data.
//!
//! Uses pgvector for storage and cosine-distance similarity search.

use sea_orm::{ConnectionTrait, DatabaseConnection, FromQueryResult, Statement};
use uuid::Uuid;

/// Embedding dimensionality.
pub const EMBEDDING_DIM: usize = 32;

/// Generate a 32-dimensional embedding vector from track metadata.
///
/// Dimension layout:
/// - `[0..12]` — Genre hash buckets (12 dims)
/// - `[12..16]` — Decade/era encoding (4 dims)
/// - `[16..20]` — Duration profile (4 dims)
/// - `[20..24]` — Audio quality profile (4 dims)
/// - `[24..28]` — Popularity signal (4 dims)
/// - `[28..32]` — Artist hash + metadata (4 dims)
#[allow(clippy::too_many_arguments)]
pub fn generate_embedding(
    genre: Option<&str>,
    year: Option<i16>,
    duration_secs: f32,
    bitrate: Option<i32>,
    sample_rate: Option<i32>,
    format: &str,
    play_count: i64,
    artist_id: Uuid,
) -> Vec<f32> {
    let mut vec = vec![0.0f32; EMBEDDING_DIM];

    // ─── Dims 0-11: Genre hash buckets ──────────────────────────
    if let Some(genre) = genre {
        let genre_lower = genre.to_lowercase();
        // Hash genre into primary bucket
        let hash = simple_hash(&genre_lower);
        let primary = (hash % 12) as usize;
        vec[primary] = 1.0;
        // Spread to adjacent bucket for smoothing
        let adjacent = ((hash / 12) % 12) as usize;
        if adjacent != primary {
            vec[adjacent] = 0.3;
        }
    }

    // ─── Dims 12-15: Decade/era encoding ────────────────────────
    if let Some(year) = year {
        let (d12, d13, d14, d15) = encode_decade(year);
        vec[12] = d12;
        vec[13] = d13;
        vec[14] = d14;
        vec[15] = d15;
    } else {
        // Unknown year: neutral center
        vec[12] = 0.25;
        vec[13] = 0.25;
        vec[14] = 0.25;
        vec[15] = 0.25;
    }

    // ─── Dims 16-19: Duration profile ───────────────────────────
    let (d16, d17, d18, d19) = encode_duration(duration_secs);
    vec[16] = d16;
    vec[17] = d17;
    vec[18] = d18;
    vec[19] = d19;

    // ─── Dims 20-23: Audio quality ──────────────────────────────
    let (d20, d21, d22, d23) = encode_audio_quality(bitrate, sample_rate, format);
    vec[20] = d20;
    vec[21] = d21;
    vec[22] = d22;
    vec[23] = d23;

    // ─── Dims 24-27: Popularity ─────────────────────────────────
    let (d24, d25, d26, d27) = encode_popularity(play_count);
    vec[24] = d24;
    vec[25] = d25;
    vec[26] = d26;
    vec[27] = d27;

    // ─── Dims 28-31: Artist hash + metadata ─────────────────────
    let artist_hash = simple_hash(&artist_id.to_string());
    vec[28] = ((artist_hash % 100) as f32) / 100.0;
    vec[29] = (((artist_hash / 100) % 100) as f32) / 100.0;
    vec[30] = if format == "flac" || format == "wav" {
        1.0
    } else {
        0.0
    }; // lossless flag
    vec[31] = if genre.is_some() && year.is_some() {
        1.0
    } else {
        0.5
    }; // metadata completeness

    // L2 normalize for cosine distance
    l2_normalize(&mut vec);

    vec
}

/// Simple deterministic hash for strings (FNV-1a inspired).
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// L2-normalize a vector in place.
fn l2_normalize(vec: &mut [f32]) {
    let magnitude: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if magnitude > 1e-10 {
        for x in vec.iter_mut() {
            *x /= magnitude;
        }
    }
}

/// Encode year into 4-dimensional era representation.
fn encode_decade(year: i16) -> (f32, f32, f32, f32) {
    match year {
        ..=1979 => (1.0, 0.0, 0.0, 0.0), // Classic
        1980..=1999 => {
            let t = (year - 1980) as f32 / 20.0;
            (1.0 - t, t, 0.0, 0.0) // Classic → Retro blend
        }
        2000..=2014 => {
            let t = (year - 2000) as f32 / 15.0;
            (0.0, 1.0 - t, t, 0.0) // Retro → Modern blend
        }
        2015.. => {
            let t = ((year - 2015) as f32 / 10.0).min(1.0);
            (0.0, 0.0, 1.0 - t, t) // Modern → Contemporary blend
        }
    }
}

/// Encode duration into 4-dimensional profile.
fn encode_duration(secs: f32) -> (f32, f32, f32, f32) {
    match secs {
        s if s < 120.0 => {
            // Short (< 2 min)
            let t = s / 120.0;
            (1.0 - t * 0.5, t * 0.5, 0.0, 0.0)
        }
        s if s < 300.0 => {
            // Medium (2-5 min) — most common
            let t = (s - 120.0) / 180.0;
            (0.0, 1.0 - t * 0.3, t * 0.3, 0.0)
        }
        s if s < 600.0 => {
            // Long (5-10 min)
            let t = (s - 300.0) / 300.0;
            (0.0, 0.0, 1.0 - t * 0.5, t * 0.5)
        }
        _ => {
            // Epic (10+ min)
            (0.0, 0.0, 0.1, 0.9)
        }
    }
}

/// Encode audio quality into 4 dimensions.
fn encode_audio_quality(
    bitrate: Option<i32>,
    sample_rate: Option<i32>,
    format: &str,
) -> (f32, f32, f32, f32) {
    let bitrate_norm = match bitrate {
        Some(br) => (br as f32 / 320.0).clamp(0.0, 1.0), // Normalized to 320kbps
        None => 0.5,
    };

    let sample_rate_norm = match sample_rate {
        Some(sr) => ((sr as f32 - 22050.0) / (96000.0 - 22050.0)).clamp(0.0, 1.0),
        None => 0.5,
    };

    let format_quality = match format {
        "flac" | "wav" | "alac" => 1.0,
        "aac" | "ogg" | "opus" => 0.7,
        "mp3" => 0.5,
        _ => 0.3,
    };

    let is_hires = if sample_rate.unwrap_or(0) > 48000 || bitrate.unwrap_or(0) > 320 {
        1.0
    } else {
        0.0
    };

    (bitrate_norm, sample_rate_norm, format_quality, is_hires)
}

/// Encode popularity (play count) into 4 dimensions using log scaling.
fn encode_popularity(play_count: i64) -> (f32, f32, f32, f32) {
    let log_plays = if play_count > 0 {
        (play_count as f64).ln() as f32
    } else {
        0.0
    };

    // Normalize log plays (ln(1000) ≈ 6.9)
    let normalized = (log_plays / 7.0).min(1.0);

    // Bucket: unplayed, low, medium, high
    match play_count {
        0 => (1.0, 0.0, 0.0, 0.0),
        1..=10 => (0.0, 1.0 - normalized, normalized, 0.0),
        11..=100 => (0.0, 0.0, 1.0 - (normalized - 0.3), normalized - 0.3),
        _ => (0.0, 0.0, 0.1, normalized),
    }
}

// ─── Database Operations ────────────────────────────────────────────

/// Format a vector as pgvector-compatible string: `[0.1,0.2,...]`
pub fn vec_to_pgvector(vec: &[f32]) -> String {
    let parts: Vec<String> = vec.iter().map(|v| format!("{v:.6}")).collect();
    format!("[{}]", parts.join(","))
}

/// Store a track embedding in the database.
///
/// Uses `INSERT ... ON CONFLICT` to upsert — if the track already has
/// an embedding, it is updated.
pub async fn upsert_track_embedding(
    db: &DatabaseConnection,
    track_id: Uuid,
    embedding: &[f32],
    metadata: Option<serde_json::Value>,
) -> Result<(), sea_orm::DbErr> {
    let vec_str = vec_to_pgvector(embedding);
    let metadata_str =
        serde_json::to_string(&metadata.unwrap_or(serde_json::json!({}))).unwrap_or_default();

    db.execute(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO track_embeddings (track_id, embedding, metadata, created_at, updated_at) \
         VALUES ($1, $2::vector, $3::jsonb, NOW(), NOW()) \
         ON CONFLICT (track_id) DO UPDATE SET \
         embedding = $2::vector, metadata = $3::jsonb, updated_at = NOW()",
        [track_id.into(), vec_str.into(), metadata_str.into()],
    ))
    .await?;

    Ok(())
}

/// Find tracks most similar to a given embedding using cosine distance.
///
/// Returns `(track_id, distance)` pairs sorted by similarity (closest first).
pub async fn find_similar_tracks(
    db: &DatabaseConnection,
    embedding: &[f32],
    limit: u64,
    exclude_ids: &[Uuid],
) -> Result<Vec<(Uuid, f64)>, sea_orm::DbErr> {
    #[derive(Debug, FromQueryResult)]
    struct SimilarRow {
        track_id: Uuid,
        distance: f64,
    }

    let vec_str = vec_to_pgvector(embedding);

    // Build exclude clause
    let exclude_clause = if exclude_ids.is_empty() {
        String::new()
    } else {
        let ids: Vec<String> = exclude_ids.iter().map(|id| format!("'{id}'")).collect();
        format!("AND track_id NOT IN ({})", ids.join(","))
    };

    let sql = format!(
        "SELECT track_id, embedding <=> $1::vector AS distance \
         FROM track_embeddings \
         WHERE 1=1 {exclude_clause} \
         ORDER BY distance ASC \
         LIMIT $2"
    );

    let rows = SimilarRow::find_by_statement(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        &sql,
        [vec_str.into(), limit.into()],
    ))
    .all(db)
    .await?;

    Ok(rows.into_iter().map(|r| (r.track_id, r.distance)).collect())
}

/// Get a track's embedding from the database.
pub async fn get_track_embedding(
    db: &DatabaseConnection,
    track_id: Uuid,
) -> Result<Option<Vec<f32>>, sea_orm::DbErr> {
    #[derive(Debug, FromQueryResult)]
    struct EmbeddingRow {
        embedding_text: String,
    }

    let rows = EmbeddingRow::find_by_statement(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "SELECT embedding::text AS embedding_text FROM track_embeddings WHERE track_id = $1",
        [track_id.into()],
    ))
    .all(db)
    .await?;

    if let Some(row) = rows.first() {
        Ok(Some(parse_pgvector(&row.embedding_text)))
    } else {
        Ok(None)
    }
}

/// Parse a pgvector text representation `[0.1,0.2,...]` into a Vec<f32>.
fn parse_pgvector(text: &str) -> Vec<f32> {
    let trimmed = text.trim_start_matches('[').trim_end_matches(']');
    trimmed
        .split(',')
        .filter_map(|s| s.trim().parse::<f32>().ok())
        .collect()
}

/// Generate and store embeddings for all tracks that don't have one yet.
///
/// This is meant to be called as a background task on startup or
/// after new tracks are added.
pub async fn backfill_embeddings(db: &DatabaseConnection) -> Result<usize, sea_orm::DbErr> {
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
    use soundtime_db::entities::track;

    #[derive(Debug, FromQueryResult)]
    struct TrackIdRow {
        id: Uuid,
    }

    // Find tracks without embeddings
    let missing = TrackIdRow::find_by_statement(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "SELECT t.id FROM tracks t \
         LEFT JOIN track_embeddings te ON t.id = te.track_id \
         WHERE te.track_id IS NULL",
        [],
    ))
    .all(db)
    .await?;

    if missing.is_empty() {
        return Ok(0);
    }

    tracing::info!(count = missing.len(), "Backfilling track embeddings");

    let mut generated = 0;
    // Process in batches
    let batch_size = 100;
    for chunk in missing.chunks(batch_size) {
        let ids: Vec<Uuid> = chunk.iter().map(|r| r.id).collect();

        let tracks: Vec<track::Model> = track::Entity::find()
            .filter(track::Column::Id.is_in(ids))
            .all(db)
            .await?;

        for t in &tracks {
            let embedding = generate_embedding(
                t.genre.as_deref(),
                t.year,
                t.duration_secs,
                t.bitrate,
                t.sample_rate,
                &t.format,
                t.play_count,
                t.artist_id,
            );

            if let Err(e) = upsert_track_embedding(db, t.id, &embedding, None).await {
                tracing::warn!(track_id = %t.id, error = %e, "Failed to store track embedding");
            } else {
                generated += 1;
            }
        }
    }

    tracing::info!(generated, "Track embedding backfill complete");
    Ok(generated)
}

/// Generate and store the embedding for a single track.
///
/// Called when a new track is added to the library.
pub async fn generate_and_store_embedding(
    db: &DatabaseConnection,
    track: &soundtime_db::entities::track::Model,
) -> Result<(), sea_orm::DbErr> {
    let embedding = generate_embedding(
        track.genre.as_deref(),
        track.year,
        track.duration_secs,
        track.bitrate,
        track.sample_rate,
        &track.format,
        track.play_count,
        track.artist_id,
    );

    upsert_track_embedding(db, track.id, &embedding, None).await
}

// ─── User Taste Vectors ─────────────────────────────────────────────

/// Compute and store a user's taste vector based on their listen history.
///
/// The taste vector is a weighted average of track embeddings where
/// the weight is derived from the completion ratio (Phase 2 behavioral
/// signals). Completed listens count more than skips.
pub async fn update_user_taste_vector(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<(), sea_orm::DbErr> {
    #[derive(Debug, FromQueryResult)]
    struct ListenEmbeddingRow {
        embedding_text: String,
        weight: f64,
    }

    // Fetch the user's recent listen history joined with track embeddings.
    // Weight: completed=1.0, skipped=0.1, otherwise=0.5
    // Note: completed and skipped are Option<bool> in the schema, so use IS TRUE.
    // Only consider last 500 listens for recency.
    let rows = ListenEmbeddingRow::find_by_statement(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "SELECT te.embedding::text AS embedding_text, \
         CASE \
           WHEN lh.completed IS TRUE THEN 1.0 \
           WHEN lh.skipped IS TRUE THEN 0.1 \
           ELSE 0.5 \
         END AS weight \
         FROM listen_history lh \
         JOIN track_embeddings te ON lh.track_id = te.track_id \
         WHERE lh.user_id = $1 \
         ORDER BY lh.listened_at DESC \
         LIMIT 500",
        [user_id.into()],
    ))
    .all(db)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    // Compute weighted average
    let mut taste = vec![0.0f64; EMBEDDING_DIM];
    let mut total_weight = 0.0f64;

    for row in &rows {
        let emb = parse_pgvector(&row.embedding_text);
        if emb.len() != EMBEDDING_DIM {
            continue;
        }
        for (i, val) in emb.iter().enumerate() {
            taste[i] += (*val as f64) * row.weight;
        }
        total_weight += row.weight;
    }

    if total_weight < 1e-10 {
        return Ok(());
    }

    // Normalize by total weight
    for val in &mut taste {
        *val /= total_weight;
    }

    // L2 normalize
    let magnitude: f64 = taste.iter().map(|x| x * x).sum::<f64>().sqrt();
    if magnitude > 1e-10 {
        for val in &mut taste {
            *val /= magnitude;
        }
    }

    let taste_f32: Vec<f32> = taste.iter().map(|v| *v as f32).collect();
    let vec_str = vec_to_pgvector(&taste_f32);
    let listen_count = rows.len() as i32;

    db.execute(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO user_taste_vectors (user_id, taste_vector, listen_count, updated_at) \
         VALUES ($1, $2::vector, $3, NOW()) \
         ON CONFLICT (user_id) DO UPDATE SET \
         taste_vector = $2::vector, listen_count = $3, updated_at = NOW()",
        [user_id.into(), vec_str.into(), listen_count.into()],
    ))
    .await?;

    Ok(())
}

/// Get a user's taste vector from the database.
pub async fn get_user_taste_vector(
    db: &DatabaseConnection,
    user_id: Uuid,
) -> Result<Option<Vec<f32>>, sea_orm::DbErr> {
    #[derive(Debug, FromQueryResult)]
    struct TasteRow {
        taste_text: String,
    }

    let rows = TasteRow::find_by_statement(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "SELECT taste_vector::text AS taste_text FROM user_taste_vectors WHERE user_id = $1",
        [user_id.into()],
    ))
    .all(db)
    .await?;

    if let Some(row) = rows.first() {
        Ok(Some(parse_pgvector(&row.taste_text)))
    } else {
        Ok(None)
    }
}

/// Find tracks that match a user's taste vector.
pub async fn recommend_for_user(
    db: &DatabaseConnection,
    user_id: Uuid,
    limit: u64,
    exclude_ids: &[Uuid],
) -> Result<Vec<(Uuid, f64)>, sea_orm::DbErr> {
    let taste = get_user_taste_vector(db, user_id).await?;
    match taste {
        Some(vec) => find_similar_tracks(db, &vec, limit, exclude_ids).await,
        None => Ok(vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_embedding_dimensions() {
        let emb = generate_embedding(
            Some("Rock"),
            Some(2020),
            240.0,
            Some(320),
            Some(44100),
            "mp3",
            100,
            Uuid::new_v4(),
        );
        assert_eq!(emb.len(), EMBEDDING_DIM);
    }

    #[test]
    fn test_embedding_is_normalized() {
        let emb = generate_embedding(
            Some("Jazz"),
            Some(1995),
            180.0,
            Some(256),
            Some(44100),
            "flac",
            50,
            Uuid::new_v4(),
        );
        let magnitude: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (magnitude - 1.0).abs() < 0.01,
            "Embedding should be L2-normalized, got magnitude {magnitude}"
        );
    }

    #[test]
    fn test_similar_genres_closer() {
        let artist = Uuid::new_v4();
        let rock1 = generate_embedding(
            Some("Rock"),
            Some(2020),
            240.0,
            Some(320),
            Some(44100),
            "mp3",
            100,
            artist,
        );
        let rock2 = generate_embedding(
            Some("Rock"),
            Some(2018),
            200.0,
            Some(256),
            Some(44100),
            "mp3",
            50,
            artist,
        );
        let jazz = generate_embedding(
            Some("Jazz"),
            Some(1990),
            360.0,
            Some(320),
            Some(44100),
            "flac",
            200,
            artist,
        );

        let dist_rock = cosine_distance(&rock1, &rock2);
        let dist_jazz = cosine_distance(&rock1, &jazz);

        assert!(
            dist_rock < dist_jazz,
            "Same genre should be closer: rock-rock={dist_rock:.4} vs rock-jazz={dist_jazz:.4}"
        );
    }

    #[test]
    fn test_no_metadata_still_works() {
        let emb = generate_embedding(None, None, 0.0, None, None, "unknown", 0, Uuid::new_v4());
        assert_eq!(emb.len(), EMBEDDING_DIM);
        let magnitude: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            magnitude > 0.0,
            "Even with no metadata, embedding should have non-zero magnitude"
        );
    }

    #[test]
    fn test_vec_to_pgvector_format() {
        let vec = vec![0.1, 0.2, 0.3];
        let result = vec_to_pgvector(&vec);
        assert!(result.starts_with('['));
        assert!(result.ends_with(']'));
        assert!(result.contains("0.1"));
    }

    #[test]
    fn test_parse_pgvector_roundtrip() {
        let original = vec![0.1, 0.2, 0.3, 0.4];
        let text = vec_to_pgvector(&original);
        let parsed = parse_pgvector(&text);
        assert_eq!(parsed.len(), original.len());
        for (a, b) in original.iter().zip(parsed.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    /// Cosine distance (1 - cosine_similarity) for testing.
    fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if mag_a < 1e-10 || mag_b < 1e-10 {
            return 1.0;
        }
        1.0 - (dot / (mag_a * mag_b))
    }

    #[test]
    fn test_decade_encoding() {
        let (a, b, _, _) = encode_decade(1970);
        assert_eq!(a, 1.0);
        assert_eq!(b, 0.0);

        let (a, _, _, _) = encode_decade(2023);
        assert_eq!(a, 0.0);
    }

    #[test]
    fn test_duration_encoding() {
        let (short, _, _, _) = encode_duration(60.0);
        assert!(short > 0.5, "60s should be in 'short' bucket");

        let (_, medium, _, _) = encode_duration(210.0);
        assert!(medium > 0.5, "210s should be in 'medium' bucket");
    }
}
