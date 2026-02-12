//! LRU blob cache for P2P tracks.
//!
//! Tracks metadata (size, last access time) for blobs stored in the iroh-blobs
//! `FsStore`. When the total cached size exceeds a configurable limit, the
//! least-recently-used blobs are evicted.
//!
//! This module does NOT own the blob store — it wraps access patterns to
//! provide bounded-size caching on top of the persistent `FsStore`.

use std::collections::{HashMap, HashSet};

use iroh_blobs::store::fs::FsStore;
use iroh_blobs::{Hash, HashAndFormat};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

// ── Constants ────────────────────────────────────────────────────────

/// Default maximum cache size: 2 GB.
const DEFAULT_MAX_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

// ── Types ────────────────────────────────────────────────────────────

/// Metadata for a single cached blob.
#[derive(Debug, Clone)]
struct CacheEntry {
    /// Size of the blob in bytes.
    size: u64,
    /// Timestamp of last access (for LRU ordering).
    last_accessed: std::time::Instant,
}

/// LRU cache tracker for P2P blobs stored in an iroh-blobs `FsStore`.
///
/// Thread-safe: all mutable state is behind `RwLock` / `Mutex`.
pub struct BlobCache {
    /// Cache entries keyed by blob hash.
    entries: RwLock<HashMap<Hash, CacheEntry>>,
    /// Total size of all cached blobs in bytes.
    total_size: RwLock<u64>,
    /// Maximum cache size in bytes.
    max_size: u64,
    /// Set of hashes currently being fetched (prevents duplicate fetches).
    in_flight: Mutex<HashSet<Hash>>,
}

impl BlobCache {
    /// Create a new cache with the given maximum size in bytes.
    pub fn new(max_size: u64) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            total_size: RwLock::new(0),
            max_size,
            in_flight: Mutex::new(HashSet::new()),
        }
    }

    /// Create a cache from the `P2P_CACHE_MAX_SIZE` environment variable.
    ///
    /// Accepts values like `"2GB"`, `"512MB"`, `"1TB"`, or raw byte counts.
    /// Falls back to [`DEFAULT_MAX_CACHE_BYTES`] (2 GB) if not set or invalid.
    pub fn from_env() -> Self {
        let max_size = std::env::var("P2P_CACHE_MAX_SIZE")
            .ok()
            .and_then(|v| parse_size(&v))
            .unwrap_or(DEFAULT_MAX_CACHE_BYTES);

        info!(
            max_size_mb = max_size / (1024 * 1024),
            "P2P blob cache configured"
        );
        Self::new(max_size)
    }

    /// Record an access to a blob, adding it to the cache if not already tracked.
    ///
    /// This should be called every time a blob is read from or written to the store.
    pub async fn record_access(&self, hash: Hash, size: u64) {
        let mut entries = self.entries.write().await;
        let mut total = self.total_size.write().await;

        if let Some(entry) = entries.get_mut(&hash) {
            entry.last_accessed = std::time::Instant::now();
        } else {
            entries.insert(
                hash,
                CacheEntry {
                    size,
                    last_accessed: std::time::Instant::now(),
                },
            );
            *total += size;
            debug!(%hash, size, total = *total, "blob added to cache tracker");
        }
    }

    /// Record access and ensure a persistent tag exists in the blob store.
    ///
    /// This is the preferred method when a blob store is available. It performs
    /// the same tracking as [`record_access`](Self::record_access) and additionally
    /// creates a `p2p-cache-{hash}` tag in the `FsStore` for new entries. This tag
    /// is what [`evict_if_needed`](Self::evict_if_needed) deletes to make blobs
    /// eligible for garbage collection.
    pub async fn record_access_with_tag(&self, hash: Hash, size: u64, blob_store: &FsStore) {
        let is_new = {
            let mut entries = self.entries.write().await;
            let mut total = self.total_size.write().await;

            if let Some(entry) = entries.get_mut(&hash) {
                entry.last_accessed = std::time::Instant::now();
                false
            } else {
                entries.insert(
                    hash,
                    CacheEntry {
                        size,
                        last_accessed: std::time::Instant::now(),
                    },
                );
                *total += size;
                debug!(%hash, size, total = *total, "blob added to cache tracker");
                true
            }
        };

        if is_new {
            let tag_name = format!("p2p-cache-{}", hash);
            if let Err(e) = blob_store
                .tags()
                .set(&tag_name, HashAndFormat::raw(hash))
                .await
            {
                warn!(%hash, error = %e, "failed to create persistent tag for cached blob");
            } else {
                debug!(%hash, "persistent tag created for cached blob");
            }
        }
    }

    /// Check if a fetch for this hash is already in progress.
    /// If not, marks it as in-flight and returns `true` (caller should fetch).
    /// If yes, returns `false` (caller should wait/retry).
    pub async fn try_start_fetch(&self, hash: Hash) -> bool {
        let mut in_flight = self.in_flight.lock().await;
        in_flight.insert(hash)
    }

    /// Mark a fetch as completed (whether success or failure).
    pub async fn finish_fetch(&self, hash: Hash) {
        let mut in_flight = self.in_flight.lock().await;
        in_flight.remove(&hash);
    }

    /// Evict least-recently-used blobs until total size is within the limit.
    ///
    /// Blobs are evicted by removing their tags from the `FsStore`, which makes
    /// them eligible for garbage collection by iroh-blobs.
    /// If tag deletion fails for a particular blob, it is skipped and the error logged.
    pub async fn evict_if_needed(&self, blob_store: &FsStore) {
        let current_total = *self.total_size.read().await;
        if current_total <= self.max_size {
            return;
        }

        let mut entries = self.entries.write().await;
        let mut total = self.total_size.write().await;

        // Sort by last_accessed ascending (oldest first)
        let mut sorted: Vec<(Hash, CacheEntry)> =
            entries.iter().map(|(h, e)| (*h, e.clone())).collect();
        sorted.sort_by_key(|(_, e)| e.last_accessed);

        let mut evicted_count = 0u64;
        let mut evicted_bytes = 0u64;

        for (hash, entry) in &sorted {
            if *total <= self.max_size {
                break;
            }

            // Remove the tag for this blob hash, making it eligible for GC.
            // iroh-blobs 0.96 does not expose a public delete_blob method;
            // instead, blobs are cleaned up by garbage collection once all
            // tags referencing them are removed.
            let tag_name = format!("p2p-cache-{}", hash);
            match blob_store.tags().delete(tag_name).await {
                Ok(_) => {
                    *total -= entry.size;
                    evicted_bytes += entry.size;
                    evicted_count += 1;
                    entries.remove(hash);
                    debug!(%hash, size = entry.size, "evicted blob from cache (tag removed, pending GC)");
                }
                Err(e) => {
                    warn!(%hash, error = %e, "failed to delete tag during eviction, skipping");
                }
            }
        }

        if evicted_count > 0 {
            info!(
                evicted_count,
                evicted_mb = evicted_bytes / (1024 * 1024),
                remaining_mb = *total / (1024 * 1024),
                "LRU cache eviction complete"
            );
        }
    }

    /// Remove a specific blob from the cache tracker (without deleting from store).
    pub async fn remove(&self, hash: &Hash) {
        let mut entries = self.entries.write().await;
        let mut total = self.total_size.write().await;
        if let Some(entry) = entries.remove(hash) {
            *total -= entry.size;
        }
    }

    /// Get the current total size of cached blobs in bytes.
    pub async fn total_size(&self) -> u64 {
        *self.total_size.read().await
    }

    /// Get the configured maximum cache size in bytes.
    pub fn max_size(&self) -> u64 {
        self.max_size
    }

    /// Get the number of blobs currently tracked.
    pub async fn entry_count(&self) -> usize {
        self.entries.read().await.len()
    }
}

// ── Size parsing ─────────────────────────────────────────────────────

/// Parse a human-readable size string like `"2GB"`, `"512MB"`, `"1TB"`, or `"1073741824"`.
fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_uppercase();

    if let Ok(bytes) = s.parse::<u64>() {
        return Some(bytes);
    }

    let (num_str, multiplier) = if let Some(n) = s.strip_suffix("TB") {
        (n, 1024u64 * 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("GB") {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("MB") {
        (n, 1024u64 * 1024)
    } else if let Some(n) = s.strip_suffix("KB") {
        (n, 1024u64)
    } else {
        return None;
    };

    num_str.trim().parse::<u64>().ok().map(|n| n * multiplier)
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_bytes() {
        assert_eq!(parse_size("1073741824"), Some(1073741824));
        assert_eq!(parse_size("0"), Some(0));
    }

    #[test]
    fn test_parse_size_units() {
        assert_eq!(parse_size("1KB"), Some(1024));
        assert_eq!(parse_size("1MB"), Some(1024 * 1024));
        assert_eq!(parse_size("2GB"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("1TB"), Some(1024u64 * 1024 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_case_insensitive() {
        assert_eq!(parse_size("2gb"), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("512Mb"), Some(512 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_whitespace() {
        assert_eq!(parse_size("  2GB  "), Some(2 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("512 MB"), Some(512 * 1024 * 1024));
    }

    #[test]
    fn test_parse_size_invalid() {
        assert_eq!(parse_size("abc"), None);
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("GB"), None);
    }

    #[tokio::test]
    async fn test_cache_record_access() {
        let cache = BlobCache::new(1024 * 1024); // 1 MB
        let hash = Hash::from_bytes([1u8; 32]);

        cache.record_access(hash, 100).await;
        assert_eq!(cache.total_size().await, 100);
        assert_eq!(cache.entry_count().await, 1);

        // Second access to same hash should not increase total size
        cache.record_access(hash, 100).await;
        assert_eq!(cache.total_size().await, 100);
        assert_eq!(cache.entry_count().await, 1);
    }

    #[tokio::test]
    async fn test_cache_multiple_entries() {
        let cache = BlobCache::new(1024 * 1024);
        let h1 = Hash::from_bytes([1u8; 32]);
        let h2 = Hash::from_bytes([2u8; 32]);

        cache.record_access(h1, 100).await;
        cache.record_access(h2, 200).await;
        assert_eq!(cache.total_size().await, 300);
        assert_eq!(cache.entry_count().await, 2);
    }

    #[tokio::test]
    async fn test_cache_remove() {
        let cache = BlobCache::new(1024 * 1024);
        let hash = Hash::from_bytes([1u8; 32]);

        cache.record_access(hash, 100).await;
        assert_eq!(cache.total_size().await, 100);

        cache.remove(&hash).await;
        assert_eq!(cache.total_size().await, 0);
        assert_eq!(cache.entry_count().await, 0);
    }

    #[tokio::test]
    async fn test_in_flight_dedup() {
        let cache = BlobCache::new(1024 * 1024);
        let hash = Hash::from_bytes([1u8; 32]);

        assert!(cache.try_start_fetch(hash).await); // first: OK
        assert!(!cache.try_start_fetch(hash).await); // second: already in flight

        cache.finish_fetch(hash).await;
        assert!(cache.try_start_fetch(hash).await); // after finish: OK again
    }

    #[test]
    fn test_default_max_cache() {
        assert_eq!(DEFAULT_MAX_CACHE_BYTES, 2 * 1024 * 1024 * 1024);
    }

    #[tokio::test]
    async fn test_record_access_with_tag_creates_tag() {
        let td = tempfile::tempdir().unwrap();
        let store = FsStore::load(td.path().join("blobs")).await.unwrap();

        let cache = BlobCache::new(1024 * 1024);
        let hash = Hash::from_bytes([1u8; 32]);

        // First access should create the tag and track the entry
        cache.record_access_with_tag(hash, 100, &store).await;
        assert_eq!(cache.total_size().await, 100);
        assert_eq!(cache.entry_count().await, 1);

        // Verify the tag was created in the store
        let tag_name = format!("p2p-cache-{}", hash);
        let tag_info = store.tags().get(&tag_name).await.unwrap();
        assert!(
            tag_info.is_some(),
            "persistent tag should exist after record_access_with_tag"
        );

        // Second access to same hash should not increase total size
        // and should not attempt to re-create the tag
        cache.record_access_with_tag(hash, 100, &store).await;
        assert_eq!(cache.total_size().await, 100);
        assert_eq!(cache.entry_count().await, 1);

        store.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_evict_if_needed_removes_tagged_blobs() {
        let td = tempfile::tempdir().unwrap();
        let store = FsStore::load(td.path().join("blobs")).await.unwrap();

        // Cache with 150 byte limit — will need to evict after adding 200 bytes
        let cache = BlobCache::new(150);
        let h1 = Hash::from_bytes([1u8; 32]);
        let h2 = Hash::from_bytes([2u8; 32]);

        // Add two blobs with tags
        cache.record_access_with_tag(h1, 100, &store).await;
        // Small delay so h1 is the oldest entry
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        cache.record_access_with_tag(h2, 100, &store).await;
        assert_eq!(cache.total_size().await, 200);

        // Eviction should remove h1 (oldest) to bring total under 150
        cache.evict_if_needed(&store).await;
        assert_eq!(cache.entry_count().await, 1);
        assert_eq!(cache.total_size().await, 100);

        // h1's tag should be deleted
        let tag1 = format!("p2p-cache-{}", h1);
        let tag_info = store.tags().get(&tag1).await.unwrap();
        assert!(tag_info.is_none(), "evicted blob's tag should be removed");

        // h2's tag should still exist
        let tag2 = format!("p2p-cache-{}", h2);
        let tag_info = store.tags().get(&tag2).await.unwrap();
        assert!(tag_info.is_some(), "retained blob's tag should still exist");

        store.shutdown().await.unwrap();
    }
}
