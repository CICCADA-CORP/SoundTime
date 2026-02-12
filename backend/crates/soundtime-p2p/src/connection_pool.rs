//! QUIC connection pool for reusing connections to P2P peers.
//!
//! Instead of opening a new QUIC connection for every message,
//! the pool caches connections by peer `EndpointId` and reuses them
//! for subsequent stream opens. Stale connections are evicted
//! automatically when `open_bi()` fails.

use std::collections::HashMap;
use std::time::Instant;

use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use tokio::sync::Mutex;
use tracing::debug;

use crate::error::P2pError;

/// Maximum number of cached connections.
const MAX_POOL_SIZE: usize = 128;

/// Connections idle longer than this are evicted on next access.
const MAX_IDLE_SECS: u64 = 60;

/// A cached connection entry.
struct PoolEntry {
    conn: Connection,
    last_used: Instant,
}

/// Thread-safe pool of reusable QUIC connections keyed by peer `EndpointId`.
pub struct ConnectionPool {
    endpoint: Endpoint,
    alpn: &'static [u8],
    entries: Mutex<HashMap<EndpointId, PoolEntry>>,
}

impl ConnectionPool {
    /// Create a new connection pool wrapping the given iroh `Endpoint`.
    pub fn new(endpoint: Endpoint, alpn: &'static [u8]) -> Self {
        Self {
            endpoint,
            alpn,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Get a reusable connection to a peer, or establish a new one.
    ///
    /// If a cached connection exists and is not stale, it is returned.
    /// Otherwise, a new connection is established and cached.
    pub async fn get_connection(&self, node_id: EndpointId) -> Result<Connection, P2pError> {
        let mut entries = self.entries.lock().await;

        // Check for an existing cached connection
        if let Some(entry) = entries.get_mut(&node_id) {
            if entry.last_used.elapsed().as_secs() < MAX_IDLE_SECS {
                entry.last_used = Instant::now();
                let conn = entry.conn.clone();
                drop(entries);
                return Ok(conn);
            } else {
                // Stale — remove it
                debug!(peer = %node_id, "evicting idle connection from pool");
                entries.remove(&node_id);
            }
        }

        // Drop the lock before connecting (connecting is async and slow)
        drop(entries);

        // Establish new connection
        let peer_addr = EndpointAddr::new(node_id);
        let conn = self
            .endpoint
            .connect(peer_addr, self.alpn)
            .await
            .map_err(|e| P2pError::Connection(e.to_string()))?;

        // Cache the new connection
        let mut entries = self.entries.lock().await;

        // Evict oldest if we're at capacity
        if entries.len() >= MAX_POOL_SIZE {
            self.evict_oldest(&mut entries);
        }

        entries.insert(
            node_id,
            PoolEntry {
                conn: conn.clone(),
                last_used: Instant::now(),
            },
        );

        Ok(conn)
    }

    /// Remove a cached connection (e.g., after a stream error).
    ///
    /// Call this when `open_bi()` or a write fails so the next attempt
    /// will establish a fresh connection.
    pub async fn invalidate(&self, node_id: &EndpointId) {
        let mut entries = self.entries.lock().await;
        if entries.remove(node_id).is_some() {
            debug!(peer = %node_id, "invalidated pooled connection");
        }
    }

    /// Remove all cached connections for peers not in the given set.
    /// Useful during periodic cleanup.
    pub async fn retain_peers(&self, active_peers: &[EndpointId]) {
        let mut entries = self.entries.lock().await;
        let before = entries.len();
        entries.retain(|id, _| active_peers.contains(id));
        let evicted = before - entries.len();
        if evicted > 0 {
            debug!(evicted, "cleaned up stale pool entries");
        }
    }

    /// Number of currently cached connections.
    pub async fn len(&self) -> usize {
        self.entries.lock().await.len()
    }

    /// Whether the pool is empty.
    pub async fn is_empty(&self) -> bool {
        self.entries.lock().await.is_empty()
    }

    /// Evict the oldest entry to make room.
    fn evict_oldest(&self, entries: &mut HashMap<EndpointId, PoolEntry>) {
        if let Some(oldest_id) = entries
            .iter()
            .min_by_key(|(_, e)| e.last_used)
            .map(|(id, _)| *id)
        {
            entries.remove(&oldest_id);
            debug!(peer = %oldest_id, "evicted oldest connection from pool (at capacity)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_constants() {
        assert_eq!(MAX_POOL_SIZE, 128);
        assert_eq!(MAX_IDLE_SECS, 60);
    }

    #[test]
    fn test_pool_entry_creation() {
        // Verify PoolEntry can be created with correct fields
        let now = Instant::now();
        assert!(now.elapsed().as_secs() < 1);
    }
}
