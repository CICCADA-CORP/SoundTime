//! QUIC connection pool for reusing connections to P2P peers.
//!
//! Instead of opening a new QUIC connection for every message,
//! the pool caches connections by peer `EndpointId` and reuses them
//! for subsequent stream opens. Stale connections are evicted
//! automatically when `open_bi()` fails.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures_util::future::{BoxFuture, FutureExt, WeakShared};
use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr, EndpointId};
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

type Dial = BoxFuture<'static, Result<Connection, Arc<str>>>;

#[derive(Default)]
struct PoolState {
    ready: HashMap<EndpointId, PoolEntry>,
    // Only the callers own the handshake. Cancelling the last waiter drops it.
    pending: HashMap<EndpointId, WeakShared<Dial>>,
}

/// Thread-safe pool of reusable QUIC connections keyed by peer `EndpointId`.
pub struct ConnectionPool {
    endpoint: Endpoint,
    alpn: &'static [u8],
    fallback: Option<&'static [u8]>,
    state: Mutex<PoolState>,
    established: AtomicU64,
}

impl ConnectionPool {
    /// Create a new connection pool wrapping the given iroh `Endpoint`.
    pub fn new(endpoint: Endpoint, alpn: &'static [u8]) -> Self {
        Self {
            endpoint,
            alpn,
            fallback: None,
            state: Mutex::default(),
            established: AtomicU64::new(0),
        }
    }

    pub fn with_fallback(mut self, fallback: &'static [u8]) -> Self {
        self.fallback = Some(fallback);
        self
    }

    /// Get a reusable connection to a peer, or establish a new one.
    ///
    /// If a cached connection exists and is not stale, it is returned.
    /// Otherwise, a new connection is established and cached.
    pub async fn get_connection(&self, node_id: EndpointId) -> Result<Connection, P2pError> {
        let dial = {
            let mut state = self.state.lock().expect("connection pool");
            if let Some(entry) = state.ready.get_mut(&node_id) {
                if entry.conn.close_reason().is_none()
                    && entry.last_used.elapsed().as_secs() < MAX_IDLE_SECS
                {
                    entry.last_used = Instant::now();
                    return Ok(entry.conn.clone());
                }
                state.ready.remove(&node_id);
            }
            if let Some(dial) = state.pending.get(&node_id).and_then(WeakShared::upgrade) {
                dial
            } else {
                if state.pending.len() >= MAX_POOL_SIZE {
                    state.pending.retain(|_, dial| dial.upgrade().is_some());
                    if state.pending.len() >= MAX_POOL_SIZE {
                        return Err(P2pError::ConnectionPoolBusy);
                    }
                }
                let endpoint = self.endpoint.clone();
                let alpn = self.alpn;
                let options = iroh::endpoint::ConnectOptions::new()
                    .with_additional_alpns(self.fallback.iter().map(|v| v.to_vec()).collect());
                let dial = async move {
                    let connecting = endpoint
                        .connect_with_opts(EndpointAddr::new(node_id), alpn, options)
                        .await
                        .map_err(|error| Arc::<str>::from(error.to_string()))?;
                    connecting
                        .await
                        .map_err(|error| Arc::<str>::from(error.to_string()))
                }
                .boxed()
                .shared();
                state
                    .pending
                    .insert(node_id, dial.downgrade().expect("unpolled handshake"));
                dial
            }
        };
        let result = dial.clone().await;
        let mut state = self.state.lock().expect("connection pool");
        // Only the first waiter publishes this exact handshake. Late waiters for
        // an older connection or error cannot replace an intervening retry.
        let current = state.pending.get(&node_id).and_then(WeakShared::upgrade);
        if current.is_some_and(|current| current.ptr_eq(&dial)) {
            state.pending.remove(&node_id);
            if let Ok(conn) = &result {
                self.established.fetch_add(1, Ordering::Relaxed);
                if state.ready.len() >= MAX_POOL_SIZE {
                    self.evict_oldest(&mut state.ready);
                }
                state.ready.insert(
                    node_id,
                    PoolEntry {
                        conn: conn.clone(),
                        last_used: Instant::now(),
                    },
                );
            }
        }
        result.map_err(|error| P2pError::Connection(error.to_string()))
    }

    /// A concurrent failure on an older stream must not evict a replacement session.
    pub fn invalidate_connection(&self, connection: &Connection) {
        let mut state = self.state.lock().expect("connection pool");
        let peer = connection.remote_id();
        if state
            .ready
            .get(&peer)
            .is_some_and(|entry| entry.conn.stable_id() == connection.stable_id())
        {
            state.ready.remove(&peer);
        }
    }

    /// Remove a cached connection (e.g., after a stream error).
    ///
    /// Call this when `open_bi()` or a write fails so the next attempt
    /// will establish a fresh connection.
    pub async fn invalidate(&self, node_id: &EndpointId) {
        let mut state = self.state.lock().expect("connection pool");
        if state.ready.remove(node_id).is_some() {
            debug!(peer = %node_id, "invalidated pooled connection");
        }
    }

    /// Remove all cached connections for peers not in the given set.
    /// Useful during periodic cleanup.
    pub async fn retain_peers(&self, active_peers: &[EndpointId]) {
        let mut state = self.state.lock().expect("connection pool");
        let before = state.ready.len();
        state.ready.retain(|id, _| active_peers.contains(id));
        let evicted = before - state.ready.len();
        if evicted > 0 {
            debug!(evicted, "cleaned up stale pool entries");
        }
    }

    /// Completed outgoing QUIC handshakes for this pool's ALPN since startup.
    pub fn established(&self) -> u64 {
        self.established.load(Ordering::Relaxed)
    }

    /// Number of currently cached connections.
    pub async fn len(&self) -> usize {
        self.state.lock().expect("connection pool").ready.len()
    }

    /// Whether the pool is empty.
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
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
pub(crate) async fn isolated_pair(
    alpn: &'static [u8],
) -> anyhow::Result<(Endpoint, ConnectionPool)> {
    use iroh::{
        address_lookup::memory::MemoryLookup,
        endpoint::{presets::Minimal, PortmapperConfig},
    };
    let base = || {
        Endpoint::builder(Minimal)
            .clear_ip_transports()
            .clear_relay_transports()
            .portmapper_config(PortmapperConfig::Disabled)
            .bind_addr("127.0.0.1:0")
    };
    let server = base()?.alpns(vec![alpn.to_vec()]).bind().await?;
    let addr = EndpointAddr::new(server.id()).with_ip_addr(server.bound_sockets()[0]);
    let client = base()?
        .address_lookup(MemoryLookup::from_endpoint_info([addr]))
        .bind()
        .await?;
    Ok((server, ConnectionPool::new(client, alpn)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_constants() {
        assert_eq!(MAX_POOL_SIZE, 128);
        assert_eq!(MAX_IDLE_SECS, 60);
    }

    #[tokio::test]
    async fn simultaneous_callers_share_a_handshake_and_can_retry_its_failure() -> anyhow::Result<()>
    {
        use anyhow::Context;
        use futures_util::future::join_all;
        use iroh::address_lookup::memory::MemoryLookup;
        use iroh::endpoint::{presets::Minimal, PortmapperConfig};
        use std::time::Duration;

        const ALPN: &[u8] = b"soundtime/pool-test";
        let lookup = MemoryLookup::new();
        let local = || {
            Endpoint::builder(Minimal)
                .clear_ip_transports()
                .clear_relay_transports()
                .portmapper_config(PortmapperConfig::Disabled)
                .address_lookup(lookup.clone())
                .alpns(vec![ALPN.to_vec()])
                .bind_addr("127.0.0.1:0")
        };
        let server = local()?.bind().await?;
        let silent = local()?.bind().await?;
        let client = local()?.bind().await?;
        for peer in [&server, &silent] {
            lookup.set_endpoint_info(
                EndpointAddr::new(peer.id()).with_ip_addr(peer.bound_sockets()[0]),
            );
        }
        let pool = ConnectionPool::new(client, ALPN);
        let silent_id = silent.id();
        let mut unfinished = Box::pin(pool.get_connection(silent_id));
        assert!(futures_util::poll!(&mut unfinished).is_pending());
        let mut second = Box::pin(pool.get_connection(silent_id));
        assert!(futures_util::poll!(&mut second).is_pending());
        drop(second);

        tokio::time::timeout(Duration::from_secs(15), async {
            let readers = || join_all((0..24).map(|_| pool.get_connection(server.id())));
            let (failed, _) = tokio::join!(readers(), async {
                server.accept().await.expect("server closed").refuse();
            });
            assert!(failed.iter().all(Result::is_err));
            assert_eq!(pool.established(), 0);

            let (connections, incoming) = tokio::join!(readers(), async {
                server
                    .accept()
                    .await
                    .context("server closed")?
                    .await
                    .map_err(anyhow::Error::from)
            });
            let _incoming = incoming?;
            let connections: Vec<_> = connections.into_iter().collect::<Result<_, _>>()?;
            assert!(connections
                .iter()
                .all(|c| c.stable_id() == connections[0].stable_id()));
            assert_eq!(pool.established(), 1);
            assert_eq!(
                pool.get_connection(server.id()).await?.stable_id(),
                connections[0].stable_id()
            );

            pool.invalidate_connection(&connections[0]);
            let (replacement, incoming) = tokio::join!(pool.get_connection(server.id()), async {
                server
                    .accept()
                    .await
                    .context("server closed")?
                    .await
                    .map_err(anyhow::Error::from)
            });
            let _incoming = incoming?;
            let replacement = replacement?;
            pool.invalidate_connection(&connections[0]);
            assert_eq!(
                pool.get_connection(server.id()).await?.stable_id(),
                replacement.stable_id()
            );
            assert_eq!(pool.established(), 2);

            // The never-accepted peer did not block the other 24 readers.
            assert!(pool.state.lock().unwrap().pending[&silent_id]
                .upgrade()
                .is_some());
            drop(unfinished);
            assert!(pool.state.lock().unwrap().pending[&silent_id]
                .upgrade()
                .is_none());
            Ok::<_, anyhow::Error>(())
        })
        .await?
    }
}
