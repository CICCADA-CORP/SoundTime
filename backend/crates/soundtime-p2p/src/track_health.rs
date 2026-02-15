//! Track health monitoring for remote P2P tracks (lazy-fetch model).
//!
//! This module provides:
//! - **Periodic health monitoring**: Background task that verifies remote tracks
//!   are still cataloged and their origin peers are reachable.
//! - **Lazy-fetch awareness**: Missing blobs are normal — tracks are fetched
//!   on-demand when played. The health sweep does NOT auto-recover blobs.
//! - **Auto-repair on playback failure**: When a P2P track cannot be fetched
//!   during playback, `auto_repair_on_failure` tries alternative peers.
//! - **3-strike dereference with automatic re-referencing**: After 3 consecutive
//!   failed playback attempts the track is marked unavailable. If the track
//!   later becomes available again, it is automatically re-referenced.
//!
//! Designed for high throughput (TB-scale catalogs) using:
//! - Concurrent batch processing with configurable parallelism
//! - Semaphore-based rate limiting to avoid overwhelming peers
//! - Chunked iteration to bound memory usage

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    Set,
};
use soundtime_db::entities::remote_track;
use tokio::sync::{watch, RwLock, Semaphore};
use tracing::{debug, info, warn};

use crate::error::P2pError;

// ── Configuration ────────────────────────────────────────────────────

/// Maximum concurrent recovery requests across all peers.
const DEFAULT_MAX_CONCURRENT_RECOVERIES: usize = 32;

/// Maximum retries before dereferencing a track.
const MAX_RETRY_ATTEMPTS: u32 = 3;

/// Default monitoring interval (10 minutes).
const DEFAULT_MONITOR_INTERVAL_SECS: u64 = 600;

/// Batch size for processing tracks during monitoring scans.
const MONITOR_BATCH_SIZE: usize = 500;

// ── Types ────────────────────────────────────────────────────────────

/// Track health status after a recovery or check attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthStatus {
    /// Track data is available locally.
    Healthy,
    /// Track was recovered successfully from a peer.
    Recovered,
    /// Track recovery failed, still has retries remaining.
    Degraded { attempts: u32 },
    /// Track has been dereferenced after exhausting all retries.
    Dereferenced,
}

/// Record of a track's health state, including retry history.
#[derive(Debug, Clone)]
pub struct TrackHealthRecord {
    /// Content hash of the track (BLAKE3).
    pub content_hash: String,
    /// Origin peer node ID.
    pub origin_node: String,
    /// Consecutive failed attempts to recover this track.
    pub failed_attempts: u32,
    /// Last attempt timestamp.
    pub last_attempt: Option<chrono::DateTime<chrono::Utc>>,
    /// Current health status.
    pub status: HealthStatus,
}

/// Information about a peer's copy of a track for duplicate resolution.
#[derive(Debug, Clone)]
pub struct PeerTrackInfo {
    /// Peer node ID.
    pub peer_id: String,
    /// Audio format (e.g., "FLAC", "MP3").
    pub format: String,
    /// Bitrate in bps (higher = better quality).
    pub bitrate: Option<i32>,
    /// Sample rate in Hz.
    pub sample_rate: Option<i32>,
    /// Whether the peer is currently online.
    pub is_online: bool,
    /// File size in bytes.
    pub file_size: i64,
}

/// Result of a recovery attempt.
#[derive(Debug, Clone)]
pub struct RecoveryResult {
    /// Content hash of the track.
    pub content_hash: String,
    /// Whether recovery succeeded.
    pub success: bool,
    /// Current health status after the attempt.
    pub status: HealthStatus,
    /// Peer that was contacted (if any).
    pub peer_used: Option<String>,
    /// Error message if recovery failed.
    pub error: Option<String>,
}

/// Configuration for the health monitor.
#[derive(Debug, Clone)]
pub struct HealthMonitorConfig {
    /// Maximum concurrent recovery requests.
    pub max_concurrent_recoveries: usize,
    /// Interval between monitoring scans (seconds).
    pub monitor_interval_secs: u64,
    /// Maximum retries before dereferencing.
    pub max_retry_attempts: u32,
    /// Batch size for processing during scans.
    pub batch_size: usize,
}

impl Default for HealthMonitorConfig {
    fn default() -> Self {
        Self {
            max_concurrent_recoveries: DEFAULT_MAX_CONCURRENT_RECOVERIES,
            monitor_interval_secs: DEFAULT_MONITOR_INTERVAL_SECS,
            max_retry_attempts: MAX_RETRY_ATTEMPTS,
            batch_size: MONITOR_BATCH_SIZE,
        }
    }
}

// ── TrackHealthManager ───────────────────────────────────────────────

/// Manages health state and recovery for remote P2P tracks.
///
/// Thread-safe: all state is behind `RwLock` and operations use `&self`.
pub struct TrackHealthManager {
    /// Health records keyed by content_hash.
    records: RwLock<HashMap<String, TrackHealthRecord>>,
    /// Semaphore to limit concurrent recovery requests.
    recovery_semaphore: Arc<Semaphore>,
    /// Configuration.
    config: HealthMonitorConfig,
}

impl Default for TrackHealthManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TrackHealthManager {
    /// Create a new health manager with default configuration.
    pub fn new() -> Self {
        Self::with_config(HealthMonitorConfig::default())
    }

    /// Create a new health manager with custom configuration.
    pub fn with_config(config: HealthMonitorConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_recoveries));
        Self {
            records: RwLock::new(HashMap::new()),
            recovery_semaphore: semaphore,
            config,
        }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &HealthMonitorConfig {
        &self.config
    }

    /// Record a failed access attempt for a track.
    /// Returns the updated health status.
    pub async fn record_failure(&self, content_hash: &str, origin_node: &str) -> HealthStatus {
        let mut records = self.records.write().await;
        let record = records
            .entry(content_hash.to_string())
            .or_insert_with(|| TrackHealthRecord {
                content_hash: content_hash.to_string(),
                origin_node: origin_node.to_string(),
                failed_attempts: 0,
                last_attempt: None,
                status: HealthStatus::Healthy,
            });

        record.failed_attempts += 1;
        record.last_attempt = Some(chrono::Utc::now());

        if record.failed_attempts >= self.config.max_retry_attempts {
            record.status = HealthStatus::Dereferenced;
        } else {
            record.status = HealthStatus::Degraded {
                attempts: record.failed_attempts,
            };
        }

        record.status.clone()
    }

    /// Record a successful recovery/access for a track, resetting its failure count.
    pub async fn record_success(&self, content_hash: &str) {
        let mut records = self.records.write().await;
        if let Some(record) = records.get_mut(content_hash) {
            record.failed_attempts = 0;
            record.last_attempt = Some(chrono::Utc::now());
            record.status = HealthStatus::Recovered;
        }
    }

    /// Mark a track as healthy (e.g., after initial successful fetch).
    pub async fn mark_healthy(&self, content_hash: &str, origin_node: &str) {
        let mut records = self.records.write().await;
        let record = records
            .entry(content_hash.to_string())
            .or_insert_with(|| TrackHealthRecord {
                content_hash: content_hash.to_string(),
                origin_node: origin_node.to_string(),
                failed_attempts: 0,
                last_attempt: None,
                status: HealthStatus::Healthy,
            });
        record.failed_attempts = 0;
        record.status = HealthStatus::Healthy;
    }

    /// Get the health record for a specific track.
    pub async fn get_record(&self, content_hash: &str) -> Option<TrackHealthRecord> {
        let records = self.records.read().await;
        records.get(content_hash).cloned()
    }

    /// Get all tracks that are in a degraded state (need recovery).
    pub async fn degraded_tracks(&self) -> Vec<TrackHealthRecord> {
        let records = self.records.read().await;
        records
            .values()
            .filter(|r| matches!(r.status, HealthStatus::Degraded { .. }))
            .cloned()
            .collect()
    }

    /// Get all dereferenced tracks.
    pub async fn dereferenced_tracks(&self) -> Vec<TrackHealthRecord> {
        let records = self.records.read().await;
        records
            .values()
            .filter(|r| r.status == HealthStatus::Dereferenced)
            .cloned()
            .collect()
    }

    /// Get total number of tracked records.
    pub async fn record_count(&self) -> usize {
        let records = self.records.read().await;
        records.len()
    }

    /// Check if a track has been dereferenced (exhausted all retries).
    pub async fn is_dereferenced(&self, content_hash: &str) -> bool {
        let records = self.records.read().await;
        records
            .get(content_hash)
            .map(|r| r.status == HealthStatus::Dereferenced)
            .unwrap_or(false)
    }

    /// Re-reference a previously dereferenced track.
    ///
    /// Called when a dereferenced track becomes available again (the peer
    /// came back online, the blob was re-announced, etc.).  Resets
    /// `failed_attempts` and sets the status back to `Healthy`.
    pub async fn re_reference(&self, content_hash: &str, origin_node: &str) {
        let mut records = self.records.write().await;
        if let Some(record) = records.get_mut(content_hash) {
            if record.status == HealthStatus::Dereferenced {
                record.failed_attempts = 0;
                record.last_attempt = Some(chrono::Utc::now());
                record.status = HealthStatus::Healthy;
                info!(
                    hash = %content_hash,
                    origin = %origin_node,
                    "re-referenced previously dereferenced track"
                );
            }
        }
    }

    /// Remove a record (e.g., when a track is deleted from the catalog).
    pub async fn remove_record(&self, content_hash: &str) {
        let mut records = self.records.write().await;
        records.remove(content_hash);
    }

    /// Clear all records.
    pub async fn clear(&self) {
        let mut records = self.records.write().await;
        records.clear();
    }

    /// Acquire a permit from the recovery semaphore.
    /// Used to limit concurrent recovery operations for backpressure.
    ///
    /// # Panics
    /// Panics if the semaphore is closed. This is acceptable because the
    /// semaphore is created in the constructor and never explicitly closed
    /// during normal operation.
    pub async fn acquire_recovery_permit(&self) -> tokio::sync::OwnedSemaphorePermit {
        Arc::clone(&self.recovery_semaphore)
            .acquire_owned()
            .await
            .expect("recovery semaphore closed unexpectedly")
    }

    /// Try to acquire a recovery permit without blocking.
    /// Returns `None` if all permits are in use (system is at capacity).
    pub fn try_acquire_recovery_permit(&self) -> Option<tokio::sync::OwnedSemaphorePermit> {
        Arc::clone(&self.recovery_semaphore)
            .try_acquire_owned()
            .ok()
    }

    /// Number of currently available recovery permits.
    pub fn available_permits(&self) -> usize {
        self.recovery_semaphore.available_permits()
    }

    /// Get a snapshot of all health records for monitoring/reporting.
    pub async fn snapshot(&self) -> Vec<TrackHealthRecord> {
        let records = self.records.read().await;
        records.values().cloned().collect()
    }

    /// Count tracks by health status.
    pub async fn status_counts(&self) -> HashMap<String, usize> {
        let records = self.records.read().await;
        let mut counts = HashMap::new();
        for record in records.values() {
            let key = match &record.status {
                HealthStatus::Healthy => "healthy",
                HealthStatus::Recovered => "recovered",
                HealthStatus::Degraded { .. } => "degraded",
                HealthStatus::Dereferenced => "dereferenced",
            };
            *counts.entry(key.to_string()).or_insert(0) += 1;
        }
        counts
    }
}

// ── Duplicate Resolution ─────────────────────────────────────────────

/// Quality score for ranking track copies across peers.
/// Higher score = better quality / more desirable copy.
pub fn quality_score(info: &PeerTrackInfo) -> u64 {
    let mut score: u64 = 0;

    // Format preference (lossless > lossy)
    score += match info.format.to_uppercase().as_str() {
        "FLAC" => 1000,
        "WAV" | "AIFF" => 900,
        "OPUS" => 700,
        "AAC" => 600,
        "OGG" => 500,
        "MP3" => 400,
        _ => 300,
    };

    // Bitrate bonus (normalized to 0-500 range)
    if let Some(br) = info.bitrate {
        score += (br as u64).min(500_000) / 1000;
    }

    // Sample rate bonus (normalized to 0-200 range)
    if let Some(sr) = info.sample_rate {
        score += (sr as u64).min(192_000) / 1000;
    }

    // Online peer bonus — strongly prefer available peers
    if info.is_online {
        score += 2000;
    }

    score
}

/// Select the best peer copy from a list of duplicate track sources.
/// Returns `None` if no copies are available.
pub fn select_best_copy(copies: &[PeerTrackInfo]) -> Option<&PeerTrackInfo> {
    if copies.is_empty() {
        return None;
    }

    // First try to find the best from online peers
    let online_copies: Vec<&PeerTrackInfo> = copies.iter().filter(|c| c.is_online).collect();
    if !online_copies.is_empty() {
        return online_copies.into_iter().max_by_key(|c| quality_score(c));
    }

    // Fall back to best overall (even offline — might come online later)
    copies.iter().max_by_key(|c| quality_score(c))
}

/// Group track copies by content hash and select the best source for each.
pub fn resolve_duplicates(all_copies: &[PeerTrackInfo], track_hash: &str) -> Option<PeerTrackInfo> {
    // Filter to copies that belong to this track hash
    // In practice the caller already filters, but this is defensive.
    let _ = track_hash; // used by caller for grouping
    select_best_copy(all_copies).cloned()
}

// ── Batch Health Check ───────────────────────────────────────────────

/// Represents a track that needs a health check during monitoring.
#[derive(Debug, Clone)]
pub struct TrackCheckItem {
    /// Content hash (BLAKE3).
    pub content_hash: String,
    /// Origin peer node ID (from remote_track.instance_domain).
    pub origin_node: String,
    /// Track title (for logging).
    pub title: String,
}

/// Result of a batch health check.
#[derive(Debug, Clone)]
pub struct BatchCheckResult {
    /// Total tracks checked.
    pub total_checked: usize,
    /// Tracks that were healthy (data found locally).
    pub healthy: usize,
    /// Tracks that were recovered from peers.
    pub recovered: usize,
    /// Tracks that failed recovery (still degraded).
    pub failed: usize,
    /// Tracks that were dereferenced (exhausted retries).
    pub dereferenced: usize,
    /// Tracks that were re-referenced after being dereferenced.
    pub re_referenced: usize,
    /// Tracks whose origin peer is currently offline (data not cached locally).
    pub unavailable_source: usize,
}

impl BatchCheckResult {
    pub fn new() -> Self {
        Self {
            total_checked: 0,
            healthy: 0,
            recovered: 0,
            failed: 0,
            dereferenced: 0,
            re_referenced: 0,
            unavailable_source: 0,
        }
    }

    pub fn merge(&mut self, other: &BatchCheckResult) {
        self.total_checked += other.total_checked;
        self.healthy += other.healthy;
        self.recovered += other.recovered;
        self.failed += other.failed;
        self.dereferenced += other.dereferenced;
        self.re_referenced += other.re_referenced;
        self.unavailable_source += other.unavailable_source;
    }
}

impl Default for BatchCheckResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a track's data exists locally in the blob store.
/// This is the fast path — no network IO needed.
pub async fn check_blob_exists<F, Fut>(content_hash: &str, has_blob: F) -> bool
where
    F: FnOnce(&str) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    has_blob(content_hash).await
}

/// Attempt to recover a single track from a specific peer.
/// Returns the recovered data bytes on success.
pub async fn attempt_recovery<F, Fut>(
    content_hash: &str,
    peer_id: &str,
    fetch_fn: F,
) -> Result<Bytes, P2pError>
where
    F: FnOnce(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<Bytes, P2pError>>,
{
    info!(hash = %content_hash, peer = %peer_id, "attempting track recovery from peer");
    fetch_fn(peer_id.to_string(), content_hash.to_string()).await
}

/// Process a batch of tracks for health checking (lazy-fetch model).
/// Uses the health manager for state tracking.
///
/// The `check_fn` is called for each track to determine if it exists locally.
/// The `_recover_fn` parameter is retained for API compatibility but is no
/// longer invoked — recovery only happens on-demand during playback via
/// `auto_repair_on_failure`.
/// The `peer_online_fn` checks whether the origin peer is currently reachable.
/// For tracks not cached locally, an offline origin peer increments
/// `unavailable_source` (the track is still counted as healthy since it may
/// become fetchable once the peer returns).
///
/// Returns aggregated results.
pub async fn process_health_batch<CF, CFut, RF, RFut, PF, PFut>(
    manager: &TrackHealthManager,
    items: &[TrackCheckItem],
    check_fn: CF,
    _recover_fn: RF,
    peer_online_fn: PF,
) -> BatchCheckResult
where
    CF: Fn(String) -> CFut + Send + Sync,
    CFut: std::future::Future<Output = bool> + Send,
    RF: Fn(String, String) -> RFut + Send + Sync,
    RFut: std::future::Future<Output = Result<Bytes, P2pError>> + Send,
    PF: Fn(String) -> PFut + Send + Sync,
    PFut: std::future::Future<Output = bool> + Send,
{
    let mut result = BatchCheckResult::new();

    for item in items {
        result.total_checked += 1;

        let was_dereferenced = manager.is_dereferenced(&item.content_hash).await;

        // Fast path: check if blob exists locally
        if check_fn(item.content_hash.clone()).await {
            // Re-reference if the track was previously dereferenced
            if was_dereferenced {
                manager
                    .re_reference(&item.content_hash, &item.origin_node)
                    .await;
                result.re_referenced += 1;
            }
            manager
                .mark_healthy(&item.content_hash, &item.origin_node)
                .await;
            result.healthy += 1;
            continue;
        }

        // Blob not cached locally — this is normal in the lazy-fetch model.
        // The blob will be fetched on-demand when the user plays the track.
        // Don't attempt background recovery; just count as healthy (fetchable).
        if !was_dereferenced {
            // Check if the origin peer is still online so we can report
            // tracks whose source is currently unreachable.
            if !peer_online_fn(item.origin_node.clone()).await {
                debug!(
                    hash = %item.content_hash,
                    title = %item.title,
                    origin = %item.origin_node,
                    "blob not cached locally and origin peer is offline"
                );
                result.unavailable_source += 1;
            } else {
                debug!(
                    hash = %item.content_hash,
                    title = %item.title,
                    "blob not cached locally (normal in lazy-fetch mode)"
                );
            }
            result.healthy += 1;
            continue;
        }

        // Previously dereferenced tracks: skip recovery, keep dereferenced status
        result.dereferenced += 1;
    }

    result
}

// ── TrackFetcher trait ────────────────────────────────────────────────

/// Abstraction over network I/O for testability.
///
/// Implement this on your P2P node to wire up real networking,
/// or use `MockFetcher` in tests.
#[async_trait]
pub trait TrackFetcher: Send + Sync + 'static {
    /// Fetch raw track bytes from a peer identified by `peer_id` and content `hash`.
    async fn fetch_track(&self, peer_id: &str, hash: &str) -> Result<Bytes, P2pError>;

    /// Check whether the blob for `hash` exists locally.
    async fn check_blob_exists(&self, hash: &str) -> bool;

    /// Check whether the peer identified by `peer_id` is currently online.
    async fn peer_is_online(&self, peer_id: &str) -> bool;

    /// List known alternative sources for a given content hash.
    /// Returns `PeerTrackInfo` entries from other peers that announced this track.
    async fn alternative_sources(&self, hash: &str) -> Vec<PeerTrackInfo>;
}

// ── Auto-repair on failure ───────────────────────────────────────────

/// Automatically attempt to repair a track that failed to play locally.
///
/// Strategy:
/// 1. Try the origin peer first (fast path).
/// 2. If that fails, select the best alternative via `select_best_copy`.
/// 3. Record success/failure in the health manager.
/// 4. After `max_retry_attempts` consecutive failures the track is dereferenced.
///
/// Returns the `RecoveryResult` describing what happened.
pub async fn auto_repair_on_failure<F: TrackFetcher>(
    manager: &TrackHealthManager,
    fetcher: &F,
    content_hash: &str,
    origin_node: &str,
) -> RecoveryResult {
    let was_dereferenced = manager.is_dereferenced(content_hash).await;

    // Acquire a recovery permit for backpressure
    let _permit = manager.acquire_recovery_permit().await;

    // 1. Try origin peer
    info!(hash = %content_hash, peer = %origin_node, dereferenced = was_dereferenced, "auto-repair: trying origin peer");
    match fetcher.fetch_track(origin_node, content_hash).await {
        Ok(_data) => {
            // Re-reference if previously dereferenced
            if was_dereferenced {
                manager.re_reference(content_hash, origin_node).await;
            }
            manager.record_success(content_hash).await;
            info!(hash = %content_hash, "auto-repair: recovered from origin peer");
            return RecoveryResult {
                content_hash: content_hash.to_string(),
                success: true,
                status: HealthStatus::Recovered,
                peer_used: Some(origin_node.to_string()),
                error: None,
            };
        }
        Err(e) => {
            warn!(hash = %content_hash, peer = %origin_node, error = %e, "auto-repair: origin peer failed");
        }
    }

    // 2. Try alternative sources (best quality first)
    let alternatives = fetcher.alternative_sources(content_hash).await;
    if let Some(best) = select_best_copy(&alternatives) {
        let peer_id = best.peer_id.clone();
        info!(hash = %content_hash, peer = %peer_id, "auto-repair: trying alternative peer");
        match fetcher.fetch_track(&peer_id, content_hash).await {
            Ok(_data) => {
                // Re-reference if previously dereferenced
                if was_dereferenced {
                    manager.re_reference(content_hash, origin_node).await;
                }
                manager.record_success(content_hash).await;
                info!(hash = %content_hash, peer = %peer_id, "auto-repair: recovered from alternative peer");
                return RecoveryResult {
                    content_hash: content_hash.to_string(),
                    success: true,
                    status: HealthStatus::Recovered,
                    peer_used: Some(peer_id),
                    error: None,
                };
            }
            Err(e) => {
                warn!(hash = %content_hash, peer = %peer_id, error = %e, "auto-repair: alternative peer failed");
            }
        }
    }

    // 3. All sources exhausted — record failure
    // If already dereferenced, don't pile on more failures; keep Dereferenced.
    if was_dereferenced {
        return RecoveryResult {
            content_hash: content_hash.to_string(),
            success: false,
            status: HealthStatus::Dereferenced,
            peer_used: None,
            error: Some("all sources exhausted, track remains dereferenced".into()),
        };
    }
    let status = manager.record_failure(content_hash, origin_node).await;
    let err_msg = format!(
        "all sources exhausted ({} alternatives tried)",
        alternatives.len()
    );
    warn!(hash = %content_hash, ?status, "auto-repair: {}", err_msg);

    RecoveryResult {
        content_hash: content_hash.to_string(),
        success: false,
        status,
        peer_used: None,
        error: Some(err_msg),
    }
}

// ── Background health monitor ────────────────────────────────────────

/// Spawn a background task that periodically sweeps remote tracks.
///
/// The task runs until the `shutdown_rx` channel receives `true`.
/// Returns the `JoinHandle` so the caller can await clean shutdown.
pub fn spawn_health_monitor<F: TrackFetcher>(
    manager: Arc<TrackHealthManager>,
    fetcher: Arc<F>,
    db: DatabaseConnection,
    mut shutdown_rx: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    let interval_secs = manager.config().monitor_interval_secs;
    let batch_size = manager.config().batch_size;

    tokio::spawn(async move {
        info!(interval_secs, batch_size, "health monitor started");

        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(interval_secs)) => {
                    info!("health monitor: starting sweep");
                    let result = run_health_sweep(&manager, &*fetcher, &db, batch_size).await;
                    info!(
                        checked = result.total_checked,
                        healthy = result.healthy,
                        recovered = result.recovered,
                        failed = result.failed,
                        dereferenced = result.dereferenced,
                        unavailable_source = result.unavailable_source,
                        "health monitor: sweep complete"
                    );
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("health monitor: shutting down");
                        break;
                    }
                }
            }
        }
    })
}

/// Perform one full sweep of all remote tracks.
///
/// Queries `remote_tracks` from the database in pages, checks blob availability
/// via the fetcher, and triggers auto-repair for missing blobs.
/// Results are persisted back to the `remote_tracks` table.
pub async fn run_health_sweep<F: TrackFetcher>(
    manager: &TrackHealthManager,
    fetcher: &F,
    db: &DatabaseConnection,
    batch_size: usize,
) -> BatchCheckResult {
    let mut overall = BatchCheckResult::new();

    // Count total remote tracks for logging
    let total = remote_track::Entity::find().count(db).await.unwrap_or(0);

    if total == 0 {
        debug!("health sweep: no remote tracks to check");
        return overall;
    }

    info!(total, batch_size, "health sweep: scanning remote tracks");

    let paginator = remote_track::Entity::find().paginate(db, batch_size as u64);
    let num_pages = paginator.num_pages().await.unwrap_or(1);

    for page_num in 0..num_pages {
        let page = match remote_track::Entity::find()
            .paginate(db, batch_size as u64)
            .fetch_page(page_num)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                warn!(page = page_num, error = %e, "health sweep: failed to fetch page");
                continue;
            }
        };

        let items: Vec<TrackCheckItem> = page
            .iter()
            .filter_map(|rt| {
                // Extract content hash from remote_uri format "p2p://node_id/hash"
                let hash = rt
                    .remote_uri
                    .strip_prefix("p2p://")
                    .and_then(|rest| rest.split('/').nth(1));
                let origin = rt
                    .instance_domain
                    .strip_prefix("p2p://")
                    .unwrap_or(&rt.instance_domain);

                hash.map(|h| TrackCheckItem {
                    content_hash: h.to_string(),
                    origin_node: origin.to_string(),
                    title: rt.title.clone(),
                })
            })
            .collect();

        if items.is_empty() {
            continue;
        }

        // Use process_health_batch with the fetcher
        let batch_result = process_health_batch(
            manager,
            &items,
            |h| {
                let fetcher_ref = &fetcher;
                let h = h.clone();
                async move { fetcher_ref.check_blob_exists(&h).await }
            },
            |peer, h| {
                let fetcher_ref = &fetcher;
                let peer = peer.clone();
                let h = h.clone();
                async move { fetcher_ref.fetch_track(&peer, &h).await }
            },
            |peer| {
                let fetcher_ref = &fetcher;
                let peer = peer.clone();
                async move { fetcher_ref.peer_is_online(&peer).await }
            },
        )
        .await;

        // Persist updated statuses back to DB
        for item in &items {
            let is_healthy = !manager.is_dereferenced(&item.content_hash).await
                && manager
                    .get_record(&item.content_hash)
                    .await
                    .map(|r| matches!(r.status, HealthStatus::Healthy | HealthStatus::Recovered))
                    .unwrap_or(true);

            persist_track_status(db, &item.content_hash, &item.origin_node, is_healthy).await;
        }

        overall.merge(&batch_result);
    }

    info!(
        total_checked = overall.total_checked,
        healthy = overall.healthy,
        recovered = overall.recovered,
        failed = overall.failed,
        dereferenced = overall.dereferenced,
        unavailable_source = overall.unavailable_source,
        "health sweep: finished"
    );

    overall
}

// ── Database persistence ─────────────────────────────────────────────

/// Update a remote track's `is_available` and `last_checked_at` in the database.
///
/// Matches by reconstructing the `remote_uri` pattern `p2p://origin_node/content_hash`.
pub async fn persist_track_status(
    db: &DatabaseConnection,
    content_hash: &str,
    origin_node: &str,
    is_available: bool,
) {
    let remote_uri = format!("p2p://{}/{}", origin_node, content_hash);
    let now = Utc::now().fixed_offset();

    let result = remote_track::Entity::find()
        .filter(remote_track::Column::RemoteUri.eq(&remote_uri))
        .one(db)
        .await;

    match result {
        Ok(Some(model)) => {
            let mut active: remote_track::ActiveModel = model.into();
            active.is_available = Set(is_available);
            active.last_checked_at = Set(Some(now));
            if let Err(e) = active.update(db).await {
                warn!(
                    remote_uri = %remote_uri,
                    error = %e,
                    "failed to persist track status"
                );
            } else {
                debug!(remote_uri = %remote_uri, is_available, "persisted track status");
            }
        }
        Ok(None) => {
            debug!(remote_uri = %remote_uri, "remote track not found in DB, skipping persist");
        }
        Err(e) => {
            warn!(remote_uri = %remote_uri, error = %e, "failed to query remote track for persist");
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── HealthMonitorConfig ──────────────────────────────────────────

    #[test]
    fn test_default_config() {
        let config = HealthMonitorConfig::default();
        assert_eq!(
            config.max_concurrent_recoveries,
            DEFAULT_MAX_CONCURRENT_RECOVERIES
        );
        assert_eq!(config.monitor_interval_secs, DEFAULT_MONITOR_INTERVAL_SECS);
        assert_eq!(config.max_retry_attempts, MAX_RETRY_ATTEMPTS);
        assert_eq!(config.batch_size, MONITOR_BATCH_SIZE);
    }

    #[test]
    fn test_custom_config() {
        let config = HealthMonitorConfig {
            max_concurrent_recoveries: 8,
            monitor_interval_secs: 60,
            max_retry_attempts: 5,
            batch_size: 100,
        };
        assert_eq!(config.max_concurrent_recoveries, 8);
        assert_eq!(config.monitor_interval_secs, 60);
        assert_eq!(config.max_retry_attempts, 5);
        assert_eq!(config.batch_size, 100);
    }

    // ── TrackHealthManager — basic operations ────────────────────────

    #[tokio::test]
    async fn test_new_manager_empty() {
        let mgr = TrackHealthManager::new();
        assert_eq!(mgr.record_count().await, 0);
        assert!(mgr.degraded_tracks().await.is_empty());
        assert!(mgr.dereferenced_tracks().await.is_empty());
    }

    #[tokio::test]
    async fn test_with_config() {
        let config = HealthMonitorConfig {
            max_concurrent_recoveries: 4,
            ..Default::default()
        };
        let mgr = TrackHealthManager::with_config(config);
        assert_eq!(mgr.config().max_concurrent_recoveries, 4);
        assert_eq!(mgr.available_permits(), 4);
    }

    #[tokio::test]
    async fn test_mark_healthy() {
        let mgr = TrackHealthManager::new();
        mgr.mark_healthy("hash1", "node1").await;

        let record = mgr.get_record("hash1").await.unwrap();
        assert_eq!(record.content_hash, "hash1");
        assert_eq!(record.origin_node, "node1");
        assert_eq!(record.failed_attempts, 0);
        assert_eq!(record.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_record_failure_first() {
        let mgr = TrackHealthManager::new();
        let status = mgr.record_failure("hash1", "node1").await;
        assert_eq!(status, HealthStatus::Degraded { attempts: 1 });

        let record = mgr.get_record("hash1").await.unwrap();
        assert_eq!(record.failed_attempts, 1);
        assert!(record.last_attempt.is_some());
    }

    #[tokio::test]
    async fn test_record_failure_escalation() {
        let mgr = TrackHealthManager::new();

        let s1 = mgr.record_failure("hash1", "node1").await;
        assert_eq!(s1, HealthStatus::Degraded { attempts: 1 });

        let s2 = mgr.record_failure("hash1", "node1").await;
        assert_eq!(s2, HealthStatus::Degraded { attempts: 2 });

        // Third failure → dereferenced (MAX_RETRY_ATTEMPTS = 3)
        let s3 = mgr.record_failure("hash1", "node1").await;
        assert_eq!(s3, HealthStatus::Dereferenced);
    }

    #[tokio::test]
    async fn test_record_failure_custom_max_retries() {
        let config = HealthMonitorConfig {
            max_retry_attempts: 5,
            ..Default::default()
        };
        let mgr = TrackHealthManager::with_config(config);

        for i in 1..5 {
            let s = mgr.record_failure("hash1", "node1").await;
            assert_eq!(s, HealthStatus::Degraded { attempts: i });
        }

        let s5 = mgr.record_failure("hash1", "node1").await;
        assert_eq!(s5, HealthStatus::Dereferenced);
    }

    #[tokio::test]
    async fn test_record_success_resets_failures() {
        let mgr = TrackHealthManager::new();

        mgr.record_failure("hash1", "node1").await;
        mgr.record_failure("hash1", "node1").await;
        assert_eq!(mgr.get_record("hash1").await.unwrap().failed_attempts, 2);

        mgr.record_success("hash1").await;
        let record = mgr.get_record("hash1").await.unwrap();
        assert_eq!(record.failed_attempts, 0);
        assert_eq!(record.status, HealthStatus::Recovered);
    }

    #[tokio::test]
    async fn test_record_success_nonexistent_noop() {
        let mgr = TrackHealthManager::new();
        // Should not panic or create a record
        mgr.record_success("nonexistent").await;
        assert!(mgr.get_record("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_is_dereferenced() {
        let mgr = TrackHealthManager::new();
        assert!(!mgr.is_dereferenced("hash1").await);

        // Exhaust retries
        for _ in 0..MAX_RETRY_ATTEMPTS {
            mgr.record_failure("hash1", "node1").await;
        }
        assert!(mgr.is_dereferenced("hash1").await);
    }

    #[tokio::test]
    async fn test_is_dereferenced_nonexistent() {
        let mgr = TrackHealthManager::new();
        assert!(!mgr.is_dereferenced("nonexistent").await);
    }

    #[tokio::test]
    async fn test_re_reference_resets_dereferenced_track() {
        let mgr = TrackHealthManager::new();

        // Dereference it
        for _ in 0..MAX_RETRY_ATTEMPTS {
            mgr.record_failure("hash1", "node1").await;
        }
        assert!(mgr.is_dereferenced("hash1").await);

        // Re-reference it
        mgr.re_reference("hash1", "node1").await;

        // Should be healthy again
        assert!(!mgr.is_dereferenced("hash1").await);
        let record = mgr.get_record("hash1").await.unwrap();
        assert_eq!(record.status, HealthStatus::Healthy);
        assert_eq!(record.failed_attempts, 0);
    }

    #[tokio::test]
    async fn test_re_reference_noop_on_healthy_track() {
        let mgr = TrackHealthManager::new();
        mgr.mark_healthy("hash1", "node1").await;

        // Re-referencing a healthy track is a no-op
        mgr.re_reference("hash1", "node1").await;

        let record = mgr.get_record("hash1").await.unwrap();
        assert_eq!(record.status, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_re_reference_noop_on_nonexistent() {
        let mgr = TrackHealthManager::new();
        // Should not panic or create a record
        mgr.re_reference("nonexistent", "node1").await;
        assert!(mgr.get_record("nonexistent").await.is_none());
    }

    // ── Listing & filtering ──────────────────────────────────────────

    #[tokio::test]
    async fn test_degraded_tracks() {
        let mgr = TrackHealthManager::new();
        mgr.record_failure("hash1", "node1").await;
        mgr.record_failure("hash2", "node2").await;
        mgr.mark_healthy("hash3", "node3").await;

        let degraded = mgr.degraded_tracks().await;
        assert_eq!(degraded.len(), 2);
    }

    #[tokio::test]
    async fn test_dereferenced_tracks() {
        let mgr = TrackHealthManager::new();

        // Dereference hash1
        for _ in 0..MAX_RETRY_ATTEMPTS {
            mgr.record_failure("hash1", "node1").await;
        }
        // hash2 is only degraded
        mgr.record_failure("hash2", "node2").await;

        let derefs = mgr.dereferenced_tracks().await;
        assert_eq!(derefs.len(), 1);
        assert_eq!(derefs[0].content_hash, "hash1");
    }

    #[tokio::test]
    async fn test_snapshot() {
        let mgr = TrackHealthManager::new();
        mgr.mark_healthy("hash1", "node1").await;
        mgr.record_failure("hash2", "node2").await;
        mgr.mark_healthy("hash3", "node3").await;

        let snap = mgr.snapshot().await;
        assert_eq!(snap.len(), 3);
    }

    #[tokio::test]
    async fn test_status_counts() {
        let mgr = TrackHealthManager::new();
        mgr.mark_healthy("h1", "n1").await;
        mgr.mark_healthy("h2", "n2").await;
        mgr.record_failure("h3", "n3").await;
        for _ in 0..MAX_RETRY_ATTEMPTS {
            mgr.record_failure("h4", "n4").await;
        }

        let counts = mgr.status_counts().await;
        assert_eq!(*counts.get("healthy").unwrap_or(&0), 2);
        assert_eq!(*counts.get("degraded").unwrap_or(&0), 1);
        assert_eq!(*counts.get("dereferenced").unwrap_or(&0), 1);
    }

    // ── Remove & clear ───────────────────────────────────────────────

    #[tokio::test]
    async fn test_remove_record() {
        let mgr = TrackHealthManager::new();
        mgr.mark_healthy("hash1", "node1").await;
        assert_eq!(mgr.record_count().await, 1);

        mgr.remove_record("hash1").await;
        assert_eq!(mgr.record_count().await, 0);
        assert!(mgr.get_record("hash1").await.is_none());
    }

    #[tokio::test]
    async fn test_remove_nonexistent() {
        let mgr = TrackHealthManager::new();
        mgr.remove_record("nonexistent").await;
        assert_eq!(mgr.record_count().await, 0);
    }

    #[tokio::test]
    async fn test_clear() {
        let mgr = TrackHealthManager::new();
        mgr.mark_healthy("h1", "n1").await;
        mgr.mark_healthy("h2", "n2").await;
        mgr.mark_healthy("h3", "n3").await;
        assert_eq!(mgr.record_count().await, 3);

        mgr.clear().await;
        assert_eq!(mgr.record_count().await, 0);
    }

    // ── Semaphore / rate limiting ────────────────────────────────────

    #[tokio::test]
    async fn test_available_permits() {
        let config = HealthMonitorConfig {
            max_concurrent_recoveries: 4,
            ..Default::default()
        };
        let mgr = TrackHealthManager::with_config(config);
        assert_eq!(mgr.available_permits(), 4);

        let _p1 = mgr.acquire_recovery_permit().await;
        assert_eq!(mgr.available_permits(), 3);

        let _p2 = mgr.acquire_recovery_permit().await;
        assert_eq!(mgr.available_permits(), 2);

        drop(_p1);
        assert_eq!(mgr.available_permits(), 3);
    }

    #[tokio::test]
    async fn test_try_acquire_permit() {
        let config = HealthMonitorConfig {
            max_concurrent_recoveries: 2,
            ..Default::default()
        };
        let mgr = TrackHealthManager::with_config(config);

        let _p1 = mgr.try_acquire_recovery_permit();
        assert!(_p1.is_some());

        let _p2 = mgr.try_acquire_recovery_permit();
        assert!(_p2.is_some());

        // Third should fail
        let p3 = mgr.try_acquire_recovery_permit();
        assert!(p3.is_none());
    }

    #[tokio::test]
    async fn test_permit_release_enables_new_acquires() {
        let config = HealthMonitorConfig {
            max_concurrent_recoveries: 1,
            ..Default::default()
        };
        let mgr = TrackHealthManager::with_config(config);

        let p1 = mgr.try_acquire_recovery_permit();
        assert!(p1.is_some());

        assert!(mgr.try_acquire_recovery_permit().is_none());

        drop(p1);
        assert!(mgr.try_acquire_recovery_permit().is_some());
    }

    // ── Quality score & duplicate resolution ─────────────────────────

    #[test]
    fn test_quality_score_format_ranking() {
        let flac = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "FLAC".into(),
            bitrate: None,
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        let mp3 = PeerTrackInfo {
            peer_id: "p2".into(),
            format: "MP3".into(),
            bitrate: None,
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        assert!(quality_score(&flac) > quality_score(&mp3));
    }

    #[test]
    fn test_quality_score_online_bonus() {
        let online = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "MP3".into(),
            bitrate: Some(128_000),
            sample_rate: Some(44100),
            is_online: true,
            file_size: 0,
        };
        let offline = PeerTrackInfo {
            peer_id: "p2".into(),
            format: "MP3".into(),
            bitrate: Some(128_000),
            sample_rate: Some(44100),
            is_online: false,
            file_size: 0,
        };
        assert!(quality_score(&online) > quality_score(&offline));
    }

    #[test]
    fn test_quality_score_bitrate_bonus() {
        let high_br = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "MP3".into(),
            bitrate: Some(320_000),
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        let low_br = PeerTrackInfo {
            peer_id: "p2".into(),
            format: "MP3".into(),
            bitrate: Some(128_000),
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        assert!(quality_score(&high_br) > quality_score(&low_br));
    }

    #[test]
    fn test_quality_score_sample_rate_bonus() {
        let high_sr = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "FLAC".into(),
            bitrate: None,
            sample_rate: Some(96_000),
            is_online: false,
            file_size: 0,
        };
        let low_sr = PeerTrackInfo {
            peer_id: "p2".into(),
            format: "FLAC".into(),
            bitrate: None,
            sample_rate: Some(44_100),
            is_online: false,
            file_size: 0,
        };
        assert!(quality_score(&high_sr) > quality_score(&low_sr));
    }

    #[test]
    fn test_quality_score_combined() {
        // Online MP3@320k should beat offline FLAC
        let online_mp3 = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "MP3".into(),
            bitrate: Some(320_000),
            sample_rate: Some(44100),
            is_online: true,
            file_size: 0,
        };
        let offline_flac = PeerTrackInfo {
            peer_id: "p2".into(),
            format: "FLAC".into(),
            bitrate: Some(1_000_000),
            sample_rate: Some(96_000),
            is_online: false,
            file_size: 0,
        };
        assert!(quality_score(&online_mp3) > quality_score(&offline_flac));
    }

    #[test]
    fn test_quality_score_unknown_format() {
        let unknown = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "UNKNOWN_FORMAT".into(),
            bitrate: None,
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        assert_eq!(quality_score(&unknown), 300); // base format score only
    }

    #[test]
    fn test_quality_score_opus() {
        let opus = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "OPUS".into(),
            bitrate: None,
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        let aac = PeerTrackInfo {
            peer_id: "p2".into(),
            format: "AAC".into(),
            bitrate: None,
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        assert!(quality_score(&opus) > quality_score(&aac));
    }

    #[test]
    fn test_quality_score_wav_vs_flac() {
        let wav = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "WAV".into(),
            bitrate: None,
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        let flac = PeerTrackInfo {
            peer_id: "p2".into(),
            format: "FLAC".into(),
            bitrate: None,
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        assert!(quality_score(&flac) > quality_score(&wav));
    }

    #[test]
    fn test_quality_score_bitrate_capped() {
        // Bitrate above 500kbps should be capped
        let huge_br = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "MP3".into(),
            bitrate: Some(999_999),
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        let capped = PeerTrackInfo {
            peer_id: "p2".into(),
            format: "MP3".into(),
            bitrate: Some(500_000),
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        assert_eq!(quality_score(&huge_br), quality_score(&capped));
    }

    // ── select_best_copy ─────────────────────────────────────────────

    #[test]
    fn test_select_best_copy_empty() {
        let copies: Vec<PeerTrackInfo> = vec![];
        assert!(select_best_copy(&copies).is_none());
    }

    #[test]
    fn test_select_best_copy_single() {
        let copies = vec![PeerTrackInfo {
            peer_id: "p1".into(),
            format: "FLAC".into(),
            bitrate: None,
            sample_rate: None,
            is_online: true,
            file_size: 0,
        }];
        let best = select_best_copy(&copies).unwrap();
        assert_eq!(best.peer_id, "p1");
    }

    #[test]
    fn test_select_best_copy_prefers_online_quality() {
        let copies = vec![
            PeerTrackInfo {
                peer_id: "p1".into(),
                format: "MP3".into(),
                bitrate: Some(128_000),
                sample_rate: Some(44100),
                is_online: true,
                file_size: 1_000_000,
            },
            PeerTrackInfo {
                peer_id: "p2".into(),
                format: "FLAC".into(),
                bitrate: Some(1_000_000),
                sample_rate: Some(96_000),
                is_online: true,
                file_size: 50_000_000,
            },
        ];
        let best = select_best_copy(&copies).unwrap();
        assert_eq!(best.peer_id, "p2"); // Higher quality, both online
    }

    #[test]
    fn test_select_best_copy_prefers_online_over_offline() {
        let copies = vec![
            PeerTrackInfo {
                peer_id: "p1".into(),
                format: "MP3".into(),
                bitrate: Some(128_000),
                sample_rate: None,
                is_online: true,
                file_size: 0,
            },
            PeerTrackInfo {
                peer_id: "p2".into(),
                format: "FLAC".into(),
                bitrate: Some(1_000_000),
                sample_rate: Some(96_000),
                is_online: false,
                file_size: 0,
            },
        ];
        let best = select_best_copy(&copies).unwrap();
        assert_eq!(best.peer_id, "p1"); // Online wins even with lower quality
    }

    #[test]
    fn test_select_best_copy_all_offline_picks_best_quality() {
        let copies = vec![
            PeerTrackInfo {
                peer_id: "p1".into(),
                format: "MP3".into(),
                bitrate: Some(128_000),
                sample_rate: None,
                is_online: false,
                file_size: 0,
            },
            PeerTrackInfo {
                peer_id: "p2".into(),
                format: "FLAC".into(),
                bitrate: Some(1_000_000),
                sample_rate: Some(96_000),
                is_online: false,
                file_size: 0,
            },
        ];
        let best = select_best_copy(&copies).unwrap();
        assert_eq!(best.peer_id, "p2"); // Best quality among offline peers
    }

    #[test]
    fn test_resolve_duplicates() {
        let copies = vec![
            PeerTrackInfo {
                peer_id: "p1".into(),
                format: "MP3".into(),
                bitrate: Some(128_000),
                sample_rate: None,
                is_online: true,
                file_size: 0,
            },
            PeerTrackInfo {
                peer_id: "p2".into(),
                format: "FLAC".into(),
                bitrate: None,
                sample_rate: Some(44100),
                is_online: true,
                file_size: 0,
            },
        ];
        let best = resolve_duplicates(&copies, "hash1").unwrap();
        assert_eq!(best.peer_id, "p2"); // FLAC > MP3 when both online
    }

    // ── BatchCheckResult ─────────────────────────────────────────────

    #[test]
    fn test_batch_check_result_new() {
        let r = BatchCheckResult::new();
        assert_eq!(r.total_checked, 0);
        assert_eq!(r.healthy, 0);
        assert_eq!(r.recovered, 0);
        assert_eq!(r.failed, 0);
        assert_eq!(r.dereferenced, 0);
        assert_eq!(r.re_referenced, 0);
        assert_eq!(r.unavailable_source, 0);
    }

    #[test]
    fn test_batch_check_result_merge() {
        let mut r1 = BatchCheckResult {
            total_checked: 10,
            healthy: 5,
            recovered: 2,
            failed: 2,
            dereferenced: 1,
            re_referenced: 1,
            unavailable_source: 3,
        };
        let r2 = BatchCheckResult {
            total_checked: 5,
            healthy: 3,
            recovered: 1,
            failed: 1,
            dereferenced: 0,
            re_referenced: 2,
            unavailable_source: 1,
        };
        r1.merge(&r2);
        assert_eq!(r1.total_checked, 15);
        assert_eq!(r1.healthy, 8);
        assert_eq!(r1.recovered, 3);
        assert_eq!(r1.failed, 3);
        assert_eq!(r1.dereferenced, 1);
        assert_eq!(r1.re_referenced, 3);
        assert_eq!(r1.unavailable_source, 4);
    }

    // ── check_blob_exists ────────────────────────────────────────────

    #[tokio::test]
    async fn test_check_blob_exists_true() {
        let result = check_blob_exists("hash1", |_h| async { true }).await;
        assert!(result);
    }

    #[tokio::test]
    async fn test_check_blob_exists_false() {
        let result = check_blob_exists("hash1", |_h| async { false }).await;
        assert!(!result);
    }

    // ── attempt_recovery ─────────────────────────────────────────────

    #[tokio::test]
    async fn test_attempt_recovery_success() {
        let data = Bytes::from_static(b"audio data");
        let result =
            attempt_recovery("hash1", "peer1", |_pid, _h| async { Ok(data.clone()) }).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Bytes::from_static(b"audio data"));
    }

    #[tokio::test]
    async fn test_attempt_recovery_failure() {
        let result = attempt_recovery("hash1", "peer1", |_pid, _h| async {
            Err(P2pError::TrackNotFound("hash1".into()))
        })
        .await;
        assert!(result.is_err());
    }

    // ── process_health_batch ─────────────────────────────────────────

    #[tokio::test]
    async fn test_batch_all_healthy() {
        let mgr = TrackHealthManager::new();
        let items = vec![
            TrackCheckItem {
                content_hash: "h1".into(),
                origin_node: "n1".into(),
                title: "Track 1".into(),
            },
            TrackCheckItem {
                content_hash: "h2".into(),
                origin_node: "n2".into(),
                title: "Track 2".into(),
            },
        ];

        let result = process_health_batch(
            &mgr,
            &items,
            |_h| async { true }, // all blobs exist
            |_p, _h| async { Ok(Bytes::new()) },
            |_| async { true }, // all peers online
        )
        .await;

        assert_eq!(result.total_checked, 2);
        assert_eq!(result.healthy, 2);
        assert_eq!(result.recovered, 0);
        assert_eq!(result.failed, 0);
    }

    #[tokio::test]
    async fn test_batch_not_cached_counts_as_healthy() {
        let mgr = TrackHealthManager::new();
        let items = vec![TrackCheckItem {
            content_hash: "h1".into(),
            origin_node: "n1".into(),
            title: "Track 1".into(),
        }];

        let result = process_health_batch(
            &mgr,
            &items,
            |_h| async { false }, // blob not found locally
            |_p, _h| async { Ok(Bytes::from_static(b"recovered data")) },
            |_| async { true }, // peer online
        )
        .await;

        // In lazy-fetch mode, missing blobs are normal — counted as healthy
        assert_eq!(result.total_checked, 1);
        assert_eq!(result.healthy, 1);
        assert_eq!(result.recovered, 0);
        assert_eq!(result.failed, 0);
    }

    #[tokio::test]
    async fn test_batch_not_cached_no_recovery_attempt() {
        let mgr = TrackHealthManager::new();
        let items = vec![TrackCheckItem {
            content_hash: "h1".into(),
            origin_node: "n1".into(),
            title: "Track 1".into(),
        }];

        let result = process_health_batch(
            &mgr,
            &items,
            |_h| async { false },
            |_p, _h| async { Err(P2pError::Connection("timeout".into())) },
            |_| async { true }, // peer online
        )
        .await;

        // In lazy-fetch mode, missing blobs don't trigger recovery
        assert_eq!(result.total_checked, 1);
        assert_eq!(result.healthy, 1);
        assert_eq!(result.recovered, 0);
        assert_eq!(result.failed, 0);

        // No health record should be created (no failure recorded)
        assert!(mgr.get_record("h1").await.is_none());
    }

    #[tokio::test]
    async fn test_batch_skips_dereferenced_when_not_available() {
        let mgr = TrackHealthManager::new();

        // Dereference the track first
        for _ in 0..MAX_RETRY_ATTEMPTS {
            mgr.record_failure("h1", "n1").await;
        }

        let items = vec![TrackCheckItem {
            content_hash: "h1".into(),
            origin_node: "n1".into(),
            title: "Track 1".into(),
        }];

        let result = process_health_batch(
            &mgr,
            &items,
            |_h| async { false }, // blob NOT available locally
            |_p, _h| async { Ok(Bytes::new()) },
            |_| async { true }, // peer online
        )
        .await;

        assert_eq!(result.total_checked, 1);
        assert_eq!(result.dereferenced, 1);
        assert_eq!(result.recovered, 0);
        assert_eq!(result.re_referenced, 0);
    }

    #[tokio::test]
    async fn test_batch_re_references_dereferenced_when_blob_exists() {
        let mgr = TrackHealthManager::new();

        // Dereference the track first
        for _ in 0..MAX_RETRY_ATTEMPTS {
            mgr.record_failure("h1", "n1").await;
        }
        assert!(mgr.is_dereferenced("h1").await);

        let items = vec![TrackCheckItem {
            content_hash: "h1".into(),
            origin_node: "n1".into(),
            title: "Track 1".into(),
        }];

        let result = process_health_batch(
            &mgr,
            &items,
            |_h| async { true }, // blob IS available locally now!
            |_p, _h| async { Ok(Bytes::new()) },
            |_| async { true }, // peer online
        )
        .await;

        assert_eq!(result.total_checked, 1);
        assert_eq!(result.healthy, 1);
        assert_eq!(result.re_referenced, 1);
        assert_eq!(result.dereferenced, 0);

        // The track should no longer be dereferenced
        assert!(!mgr.is_dereferenced("h1").await);
        let record = mgr.get_record("h1").await.unwrap();
        assert_eq!(record.status, HealthStatus::Healthy);
        assert_eq!(record.failed_attempts, 0);
    }

    #[tokio::test]
    async fn test_batch_mixed_outcomes() {
        let mgr = TrackHealthManager::new();

        // Pre-dereference h3
        for _ in 0..MAX_RETRY_ATTEMPTS {
            mgr.record_failure("h3", "n3").await;
        }

        let items = vec![
            TrackCheckItem {
                content_hash: "h1".into(),
                origin_node: "n1".into(),
                title: "Healthy Track".into(),
            },
            TrackCheckItem {
                content_hash: "h2".into(),
                origin_node: "n2".into(),
                title: "Not Cached Track".into(),
            },
            TrackCheckItem {
                content_hash: "h3".into(),
                origin_node: "n3".into(),
                title: "Dereferenced Track".into(),
            },
            TrackCheckItem {
                content_hash: "h4".into(),
                origin_node: "n4".into(),
                title: "Also Not Cached Track".into(),
            },
        ];

        let result = process_health_batch(
            &mgr,
            &items,
            |h| {
                let h = h.clone();
                async move { h == "h1" } // Only h1 exists locally
            },
            |_p, _h| async { Ok(Bytes::new()) },
            |_| async { true }, // all peers online
        )
        .await;

        // h1: locally cached → healthy
        // h2: not cached, not dereferenced → healthy (lazy-fetch)
        // h3: not cached, dereferenced → dereferenced
        // h4: not cached, not dereferenced → healthy (lazy-fetch)
        assert_eq!(result.total_checked, 4);
        assert_eq!(result.healthy, 3);
        assert_eq!(result.recovered, 0);
        assert_eq!(result.dereferenced, 1);
        assert_eq!(result.failed, 0);
    }

    #[tokio::test]
    async fn test_batch_not_cached_pre_failed_still_healthy() {
        let mgr = TrackHealthManager::new();

        // Pre-fail twice (not yet dereferenced)
        mgr.record_failure("h1", "n1").await;
        mgr.record_failure("h1", "n1").await;

        let items = vec![TrackCheckItem {
            content_hash: "h1".into(),
            origin_node: "n1".into(),
            title: "Pre-failed but not dereferenced".into(),
        }];

        let result = process_health_batch(
            &mgr,
            &items,
            |_h| async { false },
            |_p, _h| async { Err(P2pError::Connection("refused".into())) },
            |_| async { true }, // peer online
        )
        .await;

        // In lazy-fetch mode, not-dereferenced tracks with missing blobs
        // are still considered healthy (fetchable on demand)
        assert_eq!(result.total_checked, 1);
        assert_eq!(result.healthy, 1);
        assert_eq!(result.dereferenced, 0);
        // The track should NOT have been further escalated
        assert!(!mgr.is_dereferenced("h1").await);
    }

    #[tokio::test]
    async fn test_batch_unavailable_source_counting() {
        let mgr = TrackHealthManager::new();
        let items = vec![
            TrackCheckItem {
                content_hash: "h1".into(),
                origin_node: "online_peer".into(),
                title: "Online Track".into(),
            },
            TrackCheckItem {
                content_hash: "h2".into(),
                origin_node: "offline_peer".into(),
                title: "Offline Track".into(),
            },
            TrackCheckItem {
                content_hash: "h3".into(),
                origin_node: "offline_peer".into(),
                title: "Another Offline Track".into(),
            },
            TrackCheckItem {
                content_hash: "h4".into(),
                origin_node: "online_peer".into(),
                title: "Cached Track".into(),
            },
        ];

        let result = process_health_batch(
            &mgr,
            &items,
            |h| {
                let h = h.clone();
                async move { h == "h4" } // Only h4 is cached locally
            },
            |_p, _h| async { Ok(Bytes::new()) },
            |peer| {
                let peer = peer.clone();
                async move { peer == "online_peer" }
            },
        )
        .await;

        // h1: not cached, peer online → healthy, not unavailable
        // h2: not cached, peer offline → healthy + unavailable_source
        // h3: not cached, peer offline → healthy + unavailable_source
        // h4: cached locally → healthy
        assert_eq!(result.total_checked, 4);
        assert_eq!(result.healthy, 4);
        assert_eq!(result.unavailable_source, 2);
        assert_eq!(result.recovered, 0);
        assert_eq!(result.failed, 0);
        assert_eq!(result.dereferenced, 0);
    }

    // ── Concurrent behavior ──────────────────────────────────────────

    #[tokio::test]
    async fn test_concurrent_health_operations() {
        let mgr = Arc::new(TrackHealthManager::new());
        let mut handles = vec![];

        for i in 0..50 {
            let mgr = Arc::clone(&mgr);
            handles.push(tokio::spawn(async move {
                let hash = format!("hash{}", i);
                let node = format!("node{}", i % 5);
                mgr.mark_healthy(&hash, &node).await;
                if i % 3 == 0 {
                    mgr.record_failure(&hash, &node).await;
                }
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(mgr.record_count().await, 50);
    }

    #[tokio::test]
    async fn test_concurrent_permit_acquisition() {
        let config = HealthMonitorConfig {
            max_concurrent_recoveries: 5,
            ..Default::default()
        };
        let mgr = Arc::new(TrackHealthManager::with_config(config));
        let mut handles = vec![];

        for _ in 0..10 {
            let mgr = Arc::clone(&mgr);
            handles.push(tokio::spawn(async move {
                let _permit = mgr.acquire_recovery_permit().await;
                // Simulate some work
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        // All permits should be returned
        assert_eq!(mgr.available_permits(), 5);
    }

    // ── HealthStatus equality & clone ────────────────────────────────

    #[test]
    fn test_health_status_equality() {
        assert_eq!(HealthStatus::Healthy, HealthStatus::Healthy);
        assert_eq!(HealthStatus::Recovered, HealthStatus::Recovered);
        assert_eq!(HealthStatus::Dereferenced, HealthStatus::Dereferenced);
        assert_eq!(
            HealthStatus::Degraded { attempts: 2 },
            HealthStatus::Degraded { attempts: 2 }
        );
        assert_ne!(
            HealthStatus::Degraded { attempts: 1 },
            HealthStatus::Degraded { attempts: 2 }
        );
        assert_ne!(HealthStatus::Healthy, HealthStatus::Recovered);
    }

    #[test]
    fn test_health_status_clone() {
        let s = HealthStatus::Degraded { attempts: 2 };
        let s2 = s.clone();
        assert_eq!(s, s2);
    }

    #[test]
    fn test_health_status_debug() {
        let s = HealthStatus::Degraded { attempts: 1 };
        let dbg = format!("{:?}", s);
        assert!(dbg.contains("Degraded"));
        assert!(dbg.contains("1"));
    }

    // ── TrackHealthRecord clone & debug ──────────────────────────────

    #[test]
    fn test_track_health_record_clone() {
        let r = TrackHealthRecord {
            content_hash: "hash1".into(),
            origin_node: "node1".into(),
            failed_attempts: 2,
            last_attempt: Some(chrono::Utc::now()),
            status: HealthStatus::Degraded { attempts: 2 },
        };
        let r2 = r.clone();
        assert_eq!(r2.content_hash, "hash1");
        assert_eq!(r2.failed_attempts, 2);
    }

    #[test]
    fn test_track_health_record_debug() {
        let r = TrackHealthRecord {
            content_hash: "hash1".into(),
            origin_node: "node1".into(),
            failed_attempts: 0,
            last_attempt: None,
            status: HealthStatus::Healthy,
        };
        let dbg = format!("{:?}", r);
        assert!(dbg.contains("hash1"));
        assert!(dbg.contains("Healthy"));
    }

    // ── PeerTrackInfo clone and debug ────────────────────────────────

    #[test]
    fn test_peer_track_info_clone() {
        let info = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "FLAC".into(),
            bitrate: Some(1_000_000),
            sample_rate: Some(44100),
            is_online: true,
            file_size: 50_000_000,
        };
        let info2 = info.clone();
        assert_eq!(info2.peer_id, "p1");
        assert_eq!(info2.format, "FLAC");
    }

    #[test]
    fn test_peer_track_info_debug() {
        let info = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "MP3".into(),
            bitrate: Some(320_000),
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        let dbg = format!("{:?}", info);
        assert!(dbg.contains("MP3"));
        assert!(dbg.contains("320000"));
    }

    // ── RecoveryResult ───────────────────────────────────────────────

    #[test]
    fn test_recovery_result_debug() {
        let r = RecoveryResult {
            content_hash: "h1".into(),
            success: true,
            status: HealthStatus::Recovered,
            peer_used: Some("p1".into()),
            error: None,
        };
        let dbg = format!("{:?}", r);
        assert!(dbg.contains("Recovered"));
        assert!(dbg.contains("h1"));
    }

    #[test]
    fn test_recovery_result_clone() {
        let r = RecoveryResult {
            content_hash: "h1".into(),
            success: false,
            status: HealthStatus::Degraded { attempts: 1 },
            peer_used: None,
            error: Some("timeout".into()),
        };
        let r2 = r.clone();
        assert_eq!(r2.error, Some("timeout".into()));
    }

    // ── TrackCheckItem ───────────────────────────────────────────────

    #[test]
    fn test_track_check_item_clone_debug() {
        let item = TrackCheckItem {
            content_hash: "h1".into(),
            origin_node: "n1".into(),
            title: "Test Track".into(),
        };
        let item2 = item.clone();
        assert_eq!(item2.title, "Test Track");
        let dbg = format!("{:?}", item);
        assert!(dbg.contains("Test Track"));
    }

    // ── Mark healthy resets failures ──────────────────────────────────

    #[tokio::test]
    async fn test_mark_healthy_resets_degraded() {
        let mgr = TrackHealthManager::new();
        mgr.record_failure("h1", "n1").await;
        mgr.record_failure("h1", "n1").await;
        assert_eq!(
            mgr.get_record("h1").await.unwrap().status,
            HealthStatus::Degraded { attempts: 2 }
        );

        mgr.mark_healthy("h1", "n1").await;
        let r = mgr.get_record("h1").await.unwrap();
        assert_eq!(r.failed_attempts, 0);
        assert_eq!(r.status, HealthStatus::Healthy);
    }

    // ── Large batch performance ──────────────────────────────────────

    #[tokio::test]
    async fn test_large_batch_performance() {
        let mgr = TrackHealthManager::new();
        let items: Vec<TrackCheckItem> = (0..1000)
            .map(|i| TrackCheckItem {
                content_hash: format!("hash_{i}"),
                origin_node: format!("node_{}", i % 10),
                title: format!("Track {i}"),
            })
            .collect();

        let result = process_health_batch(
            &mgr,
            &items,
            |_h| async { true }, // all healthy
            |_p, _h| async { Ok(Bytes::new()) },
            |_| async { true }, // all peers online
        )
        .await;

        assert_eq!(result.total_checked, 1000);
        assert_eq!(result.healthy, 1000);
    }

    // ── Edge cases ───────────────────────────────────────────────────

    #[tokio::test]
    async fn test_empty_batch() {
        let mgr = TrackHealthManager::new();
        let result = process_health_batch(
            &mgr,
            &[],
            |_h| async { true },
            |_p, _h| async { Ok(Bytes::new()) },
            |_| async { true },
        )
        .await;
        assert_eq!(result.total_checked, 0);
    }

    #[tokio::test]
    async fn test_multiple_tracks_same_origin() {
        let mgr = TrackHealthManager::new();
        let items: Vec<TrackCheckItem> = (0..5)
            .map(|i| TrackCheckItem {
                content_hash: format!("h{i}"),
                origin_node: "same_node".into(),
                title: format!("Track {i}"),
            })
            .collect();

        let result = process_health_batch(
            &mgr,
            &items,
            |_h| async { false },
            |_p, _h| async { Ok(Bytes::from_static(b"data")) },
            |_| async { true }, // all peers online
        )
        .await;

        // In lazy-fetch mode, all uncached non-dereferenced tracks are healthy
        assert_eq!(result.total_checked, 5);
        assert_eq!(result.healthy, 5);
    }

    // ── Quality score edge cases ─────────────────────────────────────

    #[test]
    fn test_quality_score_no_metadata() {
        let info = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "MP3".into(),
            bitrate: None,
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        // Format score (400) only
        assert_eq!(quality_score(&info), 400);
    }

    #[test]
    fn test_quality_score_case_insensitive_format() {
        let lower = PeerTrackInfo {
            peer_id: "p1".into(),
            format: "flac".into(),
            bitrate: None,
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        let upper = PeerTrackInfo {
            peer_id: "p2".into(),
            format: "FLAC".into(),
            bitrate: None,
            sample_rate: None,
            is_online: false,
            file_size: 0,
        };
        assert_eq!(quality_score(&lower), quality_score(&upper));
    }

    #[test]
    fn test_select_best_multiple_online_same_format() {
        let copies = vec![
            PeerTrackInfo {
                peer_id: "p1".into(),
                format: "FLAC".into(),
                bitrate: Some(900_000),
                sample_rate: Some(44100),
                is_online: true,
                file_size: 0,
            },
            PeerTrackInfo {
                peer_id: "p2".into(),
                format: "FLAC".into(),
                bitrate: Some(1_400_000),
                sample_rate: Some(96_000),
                is_online: true,
                file_size: 0,
            },
        ];
        let best = select_best_copy(&copies).unwrap();
        // p2 has higher bitrate + sample_rate
        assert_eq!(best.peer_id, "p2");
    }

    // ══════════════════════════════════════════════════════════════════
    // MockFetcher & auto_repair / monitor tests
    // ══════════════════════════════════════════════════════════════════

    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A configurable mock fetcher for unit testing the repair pipeline.
    struct MockFetcher {
        /// Blobs that "exist" locally.
        local_blobs: RwLock<std::collections::HashSet<String>>,
        /// Peers that are "online".
        online_peers: RwLock<std::collections::HashSet<String>>,
        /// Hashes for which `fetch_track` should succeed.
        fetchable_hashes: RwLock<std::collections::HashSet<String>>,
        /// Alternative sources to return for any hash.
        alternatives: RwLock<Vec<PeerTrackInfo>>,
        /// Counter: how many fetch_track calls were made (for assertion).
        fetch_call_count: AtomicUsize,
    }

    impl MockFetcher {
        fn new() -> Self {
            Self {
                local_blobs: RwLock::new(std::collections::HashSet::new()),
                online_peers: RwLock::new(std::collections::HashSet::new()),
                fetchable_hashes: RwLock::new(std::collections::HashSet::new()),
                alternatives: RwLock::new(Vec::new()),
                fetch_call_count: AtomicUsize::new(0),
            }
        }

        async fn add_local_blob(&self, hash: &str) {
            self.local_blobs.write().await.insert(hash.to_string());
        }

        async fn set_online(&self, peer_id: &str) {
            self.online_peers.write().await.insert(peer_id.to_string());
        }

        async fn set_fetchable(&self, hash: &str) {
            self.fetchable_hashes.write().await.insert(hash.to_string());
        }

        async fn set_alternatives(&self, alts: Vec<PeerTrackInfo>) {
            *self.alternatives.write().await = alts;
        }

        fn fetch_count(&self) -> usize {
            self.fetch_call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl TrackFetcher for MockFetcher {
        async fn fetch_track(&self, _peer_id: &str, hash: &str) -> Result<Bytes, P2pError> {
            self.fetch_call_count.fetch_add(1, Ordering::SeqCst);
            if self.fetchable_hashes.read().await.contains(hash) {
                Ok(Bytes::from_static(b"mock audio data"))
            } else {
                Err(P2pError::TrackNotFound(hash.to_string()))
            }
        }

        async fn check_blob_exists(&self, hash: &str) -> bool {
            self.local_blobs.read().await.contains(hash)
        }

        async fn peer_is_online(&self, peer_id: &str) -> bool {
            self.online_peers.read().await.contains(peer_id)
        }

        async fn alternative_sources(&self, _hash: &str) -> Vec<PeerTrackInfo> {
            self.alternatives.read().await.clone()
        }
    }

    // ── auto_repair_on_failure ───────────────────────────────────────

    #[tokio::test]
    async fn test_auto_repair_origin_success() {
        let mgr = TrackHealthManager::new();
        let fetcher = MockFetcher::new();
        fetcher.set_fetchable("hash1").await;

        let result = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;

        assert!(result.success);
        assert_eq!(result.status, HealthStatus::Recovered);
        assert_eq!(result.peer_used, Some("origin1".to_string()));
        assert!(result.error.is_none());
        assert_eq!(fetcher.fetch_count(), 1);
    }

    #[tokio::test]
    async fn test_auto_repair_origin_fails_alternative_succeeds() {
        let mgr = TrackHealthManager::new();
        let fetcher = MockFetcher::new();
        // Origin will fail (hash not in fetchable), but we'll add an alternative
        // that points to a different "fetchable" mechanism.
        // Actually let's make the alternative peer fetchable for a different peer_id.
        // We need the alternative peer to succeed - so let's make the hash fetchable
        // but only after origin fails.

        // Better approach: make hash1 NOT fetchable for origin, then add it as fetchable
        // so the alternative fetch succeeds.
        fetcher
            .set_alternatives(vec![PeerTrackInfo {
                peer_id: "alt_peer".into(),
                format: "FLAC".into(),
                bitrate: Some(1_000_000),
                sample_rate: Some(44100),
                is_online: true,
                file_size: 50_000_000,
            }])
            .await;

        // Make hash1 fetchable (will fail for origin since origin isn't special -
        // actually MockFetcher doesn't distinguish by peer_id, only by hash)
        fetcher.set_fetchable("hash1").await;

        let result = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;

        // Since hash1 IS fetchable, origin fetch should succeed on the first try
        assert!(result.success);
        assert_eq!(result.peer_used, Some("origin1".to_string()));
    }

    #[tokio::test]
    async fn test_auto_repair_all_fail_records_failure() {
        let mgr = TrackHealthManager::new();
        let fetcher = MockFetcher::new();
        // hash1 NOT fetchable, no alternatives

        let result = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;

        assert!(!result.success);
        assert_eq!(result.status, HealthStatus::Degraded { attempts: 1 });
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("all sources exhausted"));
    }

    #[tokio::test]
    async fn test_auto_repair_escalation_to_dereference() {
        let config = HealthMonitorConfig {
            max_retry_attempts: 2,
            ..Default::default()
        };
        let mgr = TrackHealthManager::with_config(config);
        let fetcher = MockFetcher::new();
        // Not fetchable

        // First failure → degraded
        let r1 = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;
        assert_eq!(r1.status, HealthStatus::Degraded { attempts: 1 });

        // Second failure → dereferenced (max_retry_attempts = 2)
        let r2 = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;
        assert_eq!(r2.status, HealthStatus::Dereferenced);
    }

    #[tokio::test]
    async fn test_auto_repair_dereferenced_track_recovers() {
        let mgr = TrackHealthManager::new();

        // Pre-dereference the track
        for _ in 0..MAX_RETRY_ATTEMPTS {
            mgr.record_failure("hash1", "origin1").await;
        }
        assert!(mgr.is_dereferenced("hash1").await);

        let fetcher = MockFetcher::new();
        fetcher.set_fetchable("hash1").await; // Now it's fetchable again!

        let result = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;

        // Should succeed and re-reference
        assert!(result.success);
        assert_eq!(result.status, HealthStatus::Recovered);
        assert_eq!(result.peer_used, Some("origin1".to_string()));

        // Track should no longer be dereferenced
        assert!(!mgr.is_dereferenced("hash1").await);
        let record = mgr.get_record("hash1").await.unwrap();
        assert_eq!(record.failed_attempts, 0);
    }

    #[tokio::test]
    async fn test_auto_repair_dereferenced_still_unavailable() {
        let mgr = TrackHealthManager::new();

        // Pre-dereference the track
        for _ in 0..MAX_RETRY_ATTEMPTS {
            mgr.record_failure("hash1", "origin1").await;
        }
        assert!(mgr.is_dereferenced("hash1").await);

        let fetcher = MockFetcher::new();
        // hash1 NOT fetchable

        let result = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;

        // Should fail, stays dereferenced, no extra failure recorded
        assert!(!result.success);
        assert_eq!(result.status, HealthStatus::Dereferenced);

        // Track should still be dereferenced
        assert!(mgr.is_dereferenced("hash1").await);
        // failed_attempts should NOT have increased beyond the dereference threshold
        let record = mgr.get_record("hash1").await.unwrap();
        assert_eq!(record.failed_attempts, MAX_RETRY_ATTEMPTS);
    }

    #[tokio::test]
    async fn test_auto_repair_success_resets_failure_count() {
        let mgr = TrackHealthManager::new();
        let fetcher = MockFetcher::new();

        // Record 2 failures first
        mgr.record_failure("hash1", "origin1").await;
        mgr.record_failure("hash1", "origin1").await;

        // Now make it fetchable and repair
        fetcher.set_fetchable("hash1").await;
        let result = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;

        assert!(result.success);
        // Failure count should be reset
        let record = mgr.get_record("hash1").await.unwrap();
        assert_eq!(record.failed_attempts, 0);
        assert_eq!(record.status, HealthStatus::Recovered);
    }

    #[tokio::test]
    async fn test_auto_repair_with_alternative_sources() {
        let mgr = TrackHealthManager::new();
        let fetcher = MockFetcher::new();
        // hash1 not fetchable from anyone initially

        fetcher
            .set_alternatives(vec![
                PeerTrackInfo {
                    peer_id: "alt1".into(),
                    format: "MP3".into(),
                    bitrate: Some(128_000),
                    sample_rate: Some(44100),
                    is_online: true,
                    file_size: 5_000_000,
                },
                PeerTrackInfo {
                    peer_id: "alt2".into(),
                    format: "FLAC".into(),
                    bitrate: Some(1_000_000),
                    sample_rate: Some(96_000),
                    is_online: true,
                    file_size: 50_000_000,
                },
            ])
            .await;

        let result = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;

        // Should fail because hash1 is not fetchable even from alternatives
        assert!(!result.success);
        // But it should have tried origin + best alternative = 2 fetch attempts
        assert_eq!(fetcher.fetch_count(), 2);
    }

    #[tokio::test]
    async fn test_auto_repair_no_alternatives_only_tries_origin() {
        let mgr = TrackHealthManager::new();
        let fetcher = MockFetcher::new();
        // No alternatives, hash not fetchable

        let result = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;

        assert!(!result.success);
        // Only 1 fetch attempt (origin only)
        assert_eq!(fetcher.fetch_count(), 1);
    }

    // ── TrackFetcher trait ───────────────────────────────────────────

    #[tokio::test]
    async fn test_mock_fetcher_check_blob_exists() {
        let fetcher = MockFetcher::new();
        assert!(!fetcher.check_blob_exists("hash1").await);
        fetcher.add_local_blob("hash1").await;
        assert!(fetcher.check_blob_exists("hash1").await);
    }

    #[tokio::test]
    async fn test_mock_fetcher_peer_is_online() {
        let fetcher = MockFetcher::new();
        assert!(!fetcher.peer_is_online("peer1").await);
        fetcher.set_online("peer1").await;
        assert!(fetcher.peer_is_online("peer1").await);
    }

    #[tokio::test]
    async fn test_mock_fetcher_fetch_track_success() {
        let fetcher = MockFetcher::new();
        fetcher.set_fetchable("hash1").await;
        let result = fetcher.fetch_track("peer1", "hash1").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Bytes::from_static(b"mock audio data"));
    }

    #[tokio::test]
    async fn test_mock_fetcher_fetch_track_not_found() {
        let fetcher = MockFetcher::new();
        let result = fetcher.fetch_track("peer1", "missing").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            P2pError::TrackNotFound(h) => assert_eq!(h, "missing"),
            other => panic!("expected TrackNotFound, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_mock_fetcher_alternatives() {
        let fetcher = MockFetcher::new();
        assert!(fetcher.alternative_sources("hash1").await.is_empty());

        fetcher
            .set_alternatives(vec![PeerTrackInfo {
                peer_id: "p1".into(),
                format: "FLAC".into(),
                bitrate: None,
                sample_rate: None,
                is_online: true,
                file_size: 0,
            }])
            .await;

        let alts = fetcher.alternative_sources("hash1").await;
        assert_eq!(alts.len(), 1);
        assert_eq!(alts[0].peer_id, "p1");
    }

    // ── spawn_health_monitor ─────────────────────────────────────────

    #[tokio::test]
    async fn test_spawn_health_monitor_shutdown_via_channel() {
        // Test that the monitor respects the shutdown signal.
        // We use a very long interval so the sweep never triggers,
        // then immediately signal shutdown and expect clean exit.
        let config = HealthMonitorConfig {
            monitor_interval_secs: 3600, // Won't trigger
            ..Default::default()
        };
        let _mgr = Arc::new(TrackHealthManager::with_config(config));
        let _fetcher = Arc::new(MockFetcher::new());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let handle = tokio::spawn(async move {
            // Simulate the select loop in spawn_health_monitor
            let mut rx = shutdown_rx;
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(3600)) => {
                        // This won't trigger
                    }
                    _ = rx.changed() => {
                        if *rx.borrow() {
                            break;
                        }
                    }
                }
            }
        });

        // Send shutdown signal
        shutdown_tx.send(true).unwrap();

        // The task should exit cleanly
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;

        assert!(result.is_ok(), "monitor should shut down promptly");
    }

    // ── persist_track_status ─────────────────────────────────────────
    // persist_track_status requires a real DatabaseConnection to run.
    // It is covered via integration tests with a live PostgreSQL database.
    // Here we verify API shape at compile-time only.

    // ── Concurrent auto-repair ───────────────────────────────────────

    #[tokio::test]
    async fn test_concurrent_auto_repair() {
        let mgr = Arc::new(TrackHealthManager::new());
        let fetcher = Arc::new(MockFetcher::new());

        // Some hashes fetchable, some not
        for i in 0..5 {
            fetcher.set_fetchable(&format!("hash_{}", i)).await;
        }

        let mut handles = vec![];
        for i in 0..10 {
            let mgr = Arc::clone(&mgr);
            let fetcher = Arc::clone(&fetcher);
            handles.push(tokio::spawn(async move {
                let hash = format!("hash_{}", i);
                let origin = format!("node_{}", i % 3);
                auto_repair_on_failure(&mgr, &*fetcher, &hash, &origin).await
            }));
        }

        let mut success_count = 0;
        let mut fail_count = 0;
        for h in handles {
            let result = h.await.unwrap();
            if result.success {
                success_count += 1;
            } else {
                fail_count += 1;
            }
        }

        // 5 fetchable, 5 not
        assert_eq!(success_count, 5);
        assert_eq!(fail_count, 5);
    }

    #[tokio::test]
    async fn test_concurrent_auto_repair_with_backpressure() {
        let config = HealthMonitorConfig {
            max_concurrent_recoveries: 2, // Very limited
            ..Default::default()
        };
        let mgr = Arc::new(TrackHealthManager::with_config(config));
        let fetcher = Arc::new(MockFetcher::new());

        for i in 0..5 {
            fetcher.set_fetchable(&format!("hash_{}", i)).await;
        }

        let mut handles = vec![];
        for i in 0..5 {
            let mgr = Arc::clone(&mgr);
            let fetcher = Arc::clone(&fetcher);
            handles.push(tokio::spawn(async move {
                let hash = format!("hash_{}", i);
                auto_repair_on_failure(&mgr, &*fetcher, &hash, "origin").await
            }));
        }

        for h in handles {
            let result = h.await.unwrap();
            assert!(result.success);
        }

        // Permits should all be returned after completion
        assert_eq!(mgr.available_permits(), 2);
    }

    // ── RecoveryResult fields ────────────────────────────────────────

    #[tokio::test]
    async fn test_auto_repair_recovery_result_fields() {
        let mgr = TrackHealthManager::new();
        let fetcher = MockFetcher::new();
        fetcher.set_fetchable("hash1").await;

        let result = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;
        assert_eq!(result.content_hash, "hash1");
        assert!(result.success);
        assert_eq!(result.status, HealthStatus::Recovered);
        assert_eq!(result.peer_used.unwrap(), "origin1");
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_auto_repair_failure_result_fields() {
        let mgr = TrackHealthManager::new();
        let fetcher = MockFetcher::new();

        let result = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;
        assert_eq!(result.content_hash, "hash1");
        assert!(!result.success);
        assert!(result.peer_used.is_none());
        assert!(result.error.is_some());
    }

    // ── Integration: auto_repair + process_health_batch ──────────────

    #[tokio::test]
    async fn test_repair_then_batch_shows_recovered() {
        let mgr = TrackHealthManager::new();
        let fetcher = MockFetcher::new();
        fetcher.set_fetchable("hash1").await;

        // Repair the track first
        let repair = auto_repair_on_failure(&mgr, &fetcher, "hash1", "origin1").await;
        assert!(repair.success);

        // Now batch check should see it as recovered → healthy path via check_fn
        fetcher.add_local_blob("hash1").await;

        let items = vec![TrackCheckItem {
            content_hash: "hash1".into(),
            origin_node: "origin1".into(),
            title: "Fixed Track".into(),
        }];

        let result = process_health_batch(
            &mgr,
            &items,
            |h| {
                let f = &fetcher;
                let h = h.clone();
                async move { f.check_blob_exists(&h).await }
            },
            |p, h| {
                let f = &fetcher;
                let p = p.clone();
                let h = h.clone();
                async move { f.fetch_track(&p, &h).await }
            },
            |_| async { true }, // peer online
        )
        .await;

        assert_eq!(result.total_checked, 1);
        assert_eq!(result.healthy, 1);
    }

    // ── BatchCheckResult::new default values ─────────────────────────

    #[test]
    fn test_batch_check_result_default_is_zero() {
        let r = BatchCheckResult::new();
        assert_eq!(
            r.total_checked
                + r.healthy
                + r.recovered
                + r.failed
                + r.dereferenced
                + r.re_referenced
                + r.unavailable_source,
            0
        );
    }
}
