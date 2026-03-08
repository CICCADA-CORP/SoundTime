//! Trending tracks service with Redis sorted sets and PostgreSQL fallback.
//!
//! Uses time-bucketed Redis ZSETs (`st:trending:1h`, `st:trending:24h`,
//! `st:trending:7d`) with play-weighted scoring. Falls back to PostgreSQL
//! `listen_history` aggregation when Redis is unavailable.

use sea_orm::DatabaseConnection;
use uuid::Uuid;

/// Time window for trending queries.
#[derive(Debug, Clone, Copy)]
pub enum TrendingWindow {
    /// Last hour
    OneHour,
    /// Last 24 hours
    TwentyFourHours,
    /// Last 7 days
    SevenDays,
}

impl TrendingWindow {
    /// Redis key for this window.
    #[cfg(feature = "redis")]
    pub fn redis_key(&self) -> &'static str {
        match self {
            Self::OneHour => "st:trending:1h",
            Self::TwentyFourHours => "st:trending:24h",
            Self::SevenDays => "st:trending:7d",
        }
    }

    /// TTL for this window's Redis key (2x the window for overlap).
    #[cfg(feature = "redis")]
    pub fn ttl_secs(&self) -> u64 {
        match self {
            Self::OneHour => 7200,           // 2h
            Self::TwentyFourHours => 172800, // 48h
            Self::SevenDays => 1209600,      // 14d
        }
    }

    /// PostgreSQL interval string for fallback queries.
    pub fn pg_interval(&self) -> &'static str {
        match self {
            Self::OneHour => "1 hour",
            Self::TwentyFourHours => "24 hours",
            Self::SevenDays => "7 days",
        }
    }
}

/// A trending track with its score.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TrendingEntry {
    pub track_id: Uuid,
    pub score: f64,
}

/// Record a play event in Redis trending sorted sets.
///
/// `completed` and `skipped` determine the weight:
/// - completed: 1.0
/// - skipped: 0.1
/// - neither (partial): 0.5
#[cfg(feature = "redis")]
pub async fn record_play(
    pool: &deadpool_redis::Pool,
    track_id: Uuid,
    completed: bool,
    skipped: bool,
) {
    let weight = if completed {
        1.0
    } else if skipped {
        0.1
    } else {
        0.5
    };

    let track_key = track_id.to_string();

    // Best-effort: don't fail the request if Redis is down
    let result: Result<(), deadpool_redis::redis::RedisError> = async {
        let mut conn = pool.get().await.map_err(|e| {
            tracing::warn!("Redis connection failed: {e}");
            deadpool_redis::redis::RedisError::from((
                deadpool_redis::redis::ErrorKind::IoError,
                "pool error",
            ))
        })?;

        for window in [
            TrendingWindow::OneHour,
            TrendingWindow::TwentyFourHours,
            TrendingWindow::SevenDays,
        ] {
            let key = window.redis_key();
            let ttl = window.ttl_secs();

            // ZINCRBY key weight member
            deadpool_redis::redis::cmd("ZINCRBY")
                .arg(key)
                .arg(weight)
                .arg(&track_key)
                .query_async::<()>(&mut conn)
                .await
                .map_err(|e| {
                    tracing::warn!("Redis ZINCRBY failed for {key}: {e}");
                    e
                })?;

            // Set TTL if not already set (only on first write)
            let _: Result<(), _> = deadpool_redis::redis::cmd("EXPIRE")
                .arg(key)
                .arg(ttl)
                .arg("NX") // Only set if no TTL exists
                .query_async::<()>(&mut conn)
                .await;
        }

        Ok(())
    }
    .await;

    if let Err(e) = result {
        tracing::warn!("Failed to record trending play in Redis: {e}");
    }
}

/// Fetch trending track IDs from Redis.
#[cfg(feature = "redis")]
pub async fn fetch_trending_redis(
    pool: &deadpool_redis::Pool,
    window: TrendingWindow,
    limit: usize,
) -> Option<Vec<TrendingEntry>> {
    let mut conn = pool.get().await.ok()?;

    // ZREVRANGE key 0 limit-1 WITHSCORES → parsed as Vec<(String, f64)>
    let results: Vec<(String, f64)> = deadpool_redis::redis::cmd("ZREVRANGE")
        .arg(window.redis_key())
        .arg(0i64)
        .arg((limit as i64) - 1)
        .arg("WITHSCORES")
        .query_async(&mut conn)
        .await
        .ok()?;

    let entries: Vec<TrendingEntry> = results
        .into_iter()
        .filter_map(|(id_str, score)| {
            Uuid::parse_str(&id_str).ok().map(|track_id| TrendingEntry {
                track_id,
                score,
            })
        })
        .collect();

    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}

/// Fetch trending tracks from PostgreSQL (fallback when Redis is unavailable).
pub async fn fetch_trending_postgres(
    db: &DatabaseConnection,
    window: TrendingWindow,
    limit: u64,
) -> Result<Vec<TrendingEntry>, sea_orm::DbErr> {
    use sea_orm::{FromQueryResult, Statement};

    #[derive(Debug, FromQueryResult)]
    struct TrendingRow {
        track_id: Uuid,
        score: i64,
    }

    let interval = window.pg_interval();
    let sql = format!(
        "SELECT track_id, COUNT(*) as score \
         FROM listen_history \
         WHERE listened_at > NOW() - INTERVAL '{interval}' \
         GROUP BY track_id \
         ORDER BY score DESC \
         LIMIT $1"
    );

    let rows = TrendingRow::find_by_statement(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        &sql,
        [limit.into()],
    ))
    .all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| TrendingEntry {
            track_id: r.track_id,
            score: r.score as f64,
        })
        .collect())
}
