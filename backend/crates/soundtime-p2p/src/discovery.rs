//! Peer discovery — track known peers, announce presence, find content.
//!
//! Uses a simple registry of known peer addresses stored in the database.
//! Peers announce themselves via ping/pong and track announcements.
//! Future: integrate with iroh's built-in DNS/Pkarr discovery or DHT.

use std::collections::HashMap;
use std::sync::Arc;

use iroh::{EndpointAddr, EndpointId};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::error::P2pError;
use crate::node::{P2pMessage, P2pNode};

/// Information about a known peer.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PeerInfo {
    /// Peer's iroh EndpointId (public key)
    pub node_id: String,
    /// Human-readable name (optional, set by peer admin)
    pub name: Option<String>,
    /// Software version reported by the peer (e.g. "0.1.42")
    #[serde(default)]
    pub version: Option<String>,
    /// Number of tracks the peer has announced
    pub track_count: u64,
    /// Last time we heard from this peer (UTC timestamp)
    pub last_seen: chrono::DateTime<chrono::Utc>,
    /// Whether the peer responded to our last ping
    pub is_online: bool,
}

/// Manages the set of known peers and handles discovery.
pub struct PeerRegistry {
    /// Known peers, keyed by EndpointId string
    peers: RwLock<HashMap<String, PeerInfo>>,
}

impl PeerRegistry {
    /// Create a new empty peer registry.
    pub fn new() -> Self {
        Self {
            peers: RwLock::new(HashMap::new()),
        }
    }

    /// Register or update a peer.
    pub async fn upsert_peer(&self, node_id: &str, name: Option<String>, track_count: u64) {
        self.upsert_peer_versioned(node_id, name, track_count, None)
            .await;
    }

    /// Register or update a peer with version information.
    pub async fn upsert_peer_versioned(
        &self,
        node_id: &str,
        name: Option<String>,
        track_count: u64,
        version: Option<String>,
    ) {
        let mut peers = self.peers.write().await;
        let info = peers
            .entry(node_id.to_string())
            .or_insert_with(|| PeerInfo {
                node_id: node_id.to_string(),
                name: None,
                version: None,
                track_count: 0,
                last_seen: chrono::Utc::now(),
                is_online: true,
            });
        info.last_seen = chrono::Utc::now();
        info.is_online = true;
        info.track_count = track_count;
        if name.is_some() {
            info.name = name;
        }
        if version.is_some() {
            info.version = version;
        }
        debug!(%node_id, "peer updated in registry");
    }

    /// Mark a peer as offline.
    pub async fn mark_offline(&self, node_id: &str) {
        let mut peers = self.peers.write().await;
        if let Some(info) = peers.get_mut(node_id) {
            info.is_online = false;
        }
    }

    /// Remove a peer from the registry.
    pub async fn remove_peer(&self, node_id: &str) {
        let mut peers = self.peers.write().await;
        peers.remove(node_id);
    }

    /// Get all known peers.
    pub async fn list_peers(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers.values().cloned().collect()
    }

    /// Get online peers only.
    pub async fn online_peers(&self) -> Vec<PeerInfo> {
        let peers = self.peers.read().await;
        peers.values().filter(|p| p.is_online).cloned().collect()
    }

    /// Get a specific peer by node_id.
    pub async fn get_peer(&self, node_id: &str) -> Option<PeerInfo> {
        let peers = self.peers.read().await;
        peers.get(node_id).cloned()
    }

    /// Count of known peers.
    pub async fn peer_count(&self) -> usize {
        let peers = self.peers.read().await;
        peers.len()
    }

    /// Persist the entire peer registry to the database.
    ///
    /// Uses upsert (INSERT … ON CONFLICT UPDATE) so it is safe to call
    /// repeatedly.  Runs inside a single transaction for atomicity.
    pub async fn save_to_db(&self, db: &sea_orm::DatabaseConnection) -> Result<(), P2pError> {
        use sea_orm::{EntityTrait, Set, TransactionTrait};
        use soundtime_db::entities::p2p_peer;

        let peers = self.peers.read().await;
        let txn = db
            .begin()
            .await
            .map_err(|e| P2pError::Connection(format!("failed to begin transaction: {e}")))?;

        for info in peers.values() {
            let model = p2p_peer::ActiveModel {
                node_id: Set(info.node_id.clone()),
                name: Set(info.name.clone()),
                version: Set(info.version.clone()),
                track_count: Set(info.track_count as i64),
                is_online: Set(info.is_online),
                last_seen_at: Set(info.last_seen.into()),
                created_at: Set(chrono::Utc::now().into()),
            };
            // Insert or update on conflict (node_id is the PK)
            p2p_peer::Entity::insert(model)
                .on_conflict(
                    sea_orm::sea_query::OnConflict::column(p2p_peer::Column::NodeId)
                        .update_columns([
                            p2p_peer::Column::Name,
                            p2p_peer::Column::Version,
                            p2p_peer::Column::TrackCount,
                            p2p_peer::Column::IsOnline,
                            p2p_peer::Column::LastSeenAt,
                        ])
                        .to_owned(),
                )
                .exec(&txn)
                .await
                .map_err(|e| P2pError::Connection(format!("failed to upsert peer: {e}")))?;
        }

        txn.commit()
            .await
            .map_err(|e| P2pError::Connection(format!("failed to commit transaction: {e}")))?;

        debug!(count = peers.len(), "saved peer registry to database");
        Ok(())
    }

    /// Load peers from the database (used at startup to restore known peers).
    /// All loaded peers are marked offline until pinged.
    pub async fn load_from_db(&self, db: &sea_orm::DatabaseConnection) -> Result<usize, P2pError> {
        use sea_orm::EntityTrait;
        use soundtime_db::entities::p2p_peer;

        let rows = p2p_peer::Entity::find()
            .all(db)
            .await
            .map_err(|e| P2pError::Connection(format!("failed to load peers: {e}")))?;

        let count = rows.len();
        let mut peers = self.peers.write().await;
        for row in rows {
            let info = PeerInfo {
                node_id: row.node_id,
                name: row.name,
                version: row.version,
                track_count: row.track_count as u64,
                last_seen: row.last_seen_at.into(),
                is_online: false, // mark offline until we ping
            };
            peers.insert(info.node_id.clone(), info);
        }

        info!(count, "loaded peer registry from database");
        Ok(count)
    }
}

impl Default for PeerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Manually add a peer by its EndpointAddr and ping it.
/// Returns the PeerInfo if the peer responds.
pub async fn add_and_ping_peer(
    node: &Arc<P2pNode>,
    registry: &PeerRegistry,
    peer_addr: EndpointAddr,
) -> Result<PeerInfo, P2pError> {
    let peer_id = peer_addr.id.to_string();
    info!(%peer_id, "adding peer and pinging");

    match node.ping_peer(peer_addr).await {
        Ok(P2pMessage::Pong {
            node_id,
            track_count,
            version,
        }) => {
            registry
                .upsert_peer_versioned(&node_id, None, track_count, version)
                .await;
            let info = match registry.get_peer(&node_id).await {
                Some(info) => info,
                None => {
                    return Err(P2pError::Connection("peer disappeared after insert".into()));
                }
            };
            Ok(info)
        }
        Ok(_) => {
            warn!(%peer_id, "unexpected response to ping");
            Err(P2pError::Connection(format!(
                "unexpected response from {peer_id}"
            )))
        }
        Err(e) => {
            registry.mark_offline(&peer_id).await;
            Err(e)
        }
    }
}

/// Ping all known peers and update their status.
pub async fn refresh_all_peers(node: &Arc<P2pNode>, registry: &PeerRegistry) {
    let peers = registry.list_peers().await;
    info!(count = peers.len(), "refreshing peer statuses");

    for peer in peers {
        let node_id: EndpointId = match peer.node_id.parse() {
            Ok(id) => id,
            Err(_) => {
                warn!(peer_id = %peer.node_id, "invalid node_id in registry, removing");
                registry.remove_peer(&peer.node_id).await;
                continue;
            }
        };

        let peer_addr = EndpointAddr::new(node_id);
        match node.ping_peer(peer_addr).await {
            Ok(P2pMessage::Pong {
                node_id: _,
                track_count,
                version,
            }) => {
                registry
                    .upsert_peer_versioned(&peer.node_id, peer.name.clone(), track_count, version)
                    .await;
            }
            _ => {
                registry.mark_offline(&peer.node_id).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PeerRegistry CRUD (existing, enhanced) ───────────────────────

    #[tokio::test]
    async fn test_peer_registry_crud() {
        let registry = PeerRegistry::new();

        // Insert a peer
        registry
            .upsert_peer("peer1", Some("Alpha".to_string()), 10)
            .await;
        assert_eq!(registry.peer_count().await, 1);

        // Read it back
        let peer = registry.get_peer("peer1").await.unwrap();
        assert_eq!(peer.name, Some("Alpha".to_string()));
        assert_eq!(peer.track_count, 10);
        assert!(peer.is_online);

        // Update it
        registry.upsert_peer("peer1", None, 20).await;
        let peer = registry.get_peer("peer1").await.unwrap();
        assert_eq!(peer.track_count, 20);
        assert_eq!(peer.name, Some("Alpha".to_string())); // name preserved

        // Mark offline
        registry.mark_offline("peer1").await;
        let peer = registry.get_peer("peer1").await.unwrap();
        assert!(!peer.is_online);

        // Online peers should be empty
        assert_eq!(registry.online_peers().await.len(), 0);

        // Remove
        registry.remove_peer("peer1").await;
        assert_eq!(registry.peer_count().await, 0);
    }

    // ── PeerRegistry::new / Default ──────────────────────────────────

    #[tokio::test]
    async fn test_peer_registry_default() {
        let registry = PeerRegistry::default();
        assert_eq!(registry.peer_count().await, 0);
        assert!(registry.list_peers().await.is_empty());
        assert!(registry.online_peers().await.is_empty());
    }

    // ── get_peer: nonexistent ────────────────────────────────────────

    #[tokio::test]
    async fn test_get_peer_nonexistent() {
        let registry = PeerRegistry::new();
        assert!(registry.get_peer("ghost").await.is_none());
    }

    // ── upsert_peer: name replacement semantics ──────────────────────

    #[tokio::test]
    async fn test_upsert_peer_name_replacement() {
        let registry = PeerRegistry::new();

        // Insert with name
        registry.upsert_peer("p1", Some("Original".into()), 5).await;
        assert_eq!(
            registry.get_peer("p1").await.unwrap().name,
            Some("Original".into())
        );

        // Update with None name → should preserve existing name
        registry.upsert_peer("p1", None, 10).await;
        assert_eq!(
            registry.get_peer("p1").await.unwrap().name,
            Some("Original".into())
        );

        // Update with new name → should replace
        registry.upsert_peer("p1", Some("Renamed".into()), 15).await;
        assert_eq!(
            registry.get_peer("p1").await.unwrap().name,
            Some("Renamed".into())
        );
    }

    // ── upsert_peer: insert without name ─────────────────────────────

    #[tokio::test]
    async fn test_upsert_peer_insert_without_name() {
        let registry = PeerRegistry::new();
        registry.upsert_peer("p1", None, 0).await;
        let peer = registry.get_peer("p1").await.unwrap();
        assert!(peer.name.is_none());
        assert!(peer.is_online);
        assert_eq!(peer.track_count, 0);
    }

    // ── upsert_peer: marks peer online ───────────────────────────────

    #[tokio::test]
    async fn test_upsert_marks_online() {
        let registry = PeerRegistry::new();
        registry.upsert_peer("p1", None, 0).await;
        registry.mark_offline("p1").await;
        assert!(!registry.get_peer("p1").await.unwrap().is_online);

        // upsert should mark it online again
        registry.upsert_peer("p1", None, 5).await;
        assert!(registry.get_peer("p1").await.unwrap().is_online);
    }

    // ── upsert_peer: updates last_seen ───────────────────────────────

    #[tokio::test]
    async fn test_upsert_updates_last_seen() {
        let registry = PeerRegistry::new();
        registry.upsert_peer("p1", None, 0).await;
        let first_seen = registry.get_peer("p1").await.unwrap().last_seen;

        // Small delay to ensure time difference
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        registry.upsert_peer("p1", None, 1).await;
        let second_seen = registry.get_peer("p1").await.unwrap().last_seen;
        assert!(second_seen >= first_seen);
    }

    // ── mark_offline: nonexistent peer ───────────────────────────────

    #[tokio::test]
    async fn test_mark_offline_nonexistent() {
        let registry = PeerRegistry::new();
        // Should not panic
        registry.mark_offline("ghost").await;
        assert_eq!(registry.peer_count().await, 0);
    }

    // ── remove_peer: nonexistent ─────────────────────────────────────

    #[tokio::test]
    async fn test_remove_peer_nonexistent() {
        let registry = PeerRegistry::new();
        // Should not panic
        registry.remove_peer("ghost").await;
        assert_eq!(registry.peer_count().await, 0);
    }

    // ── list_peers: multiple peers ───────────────────────────────────

    #[tokio::test]
    async fn test_list_peers_multiple() {
        let registry = PeerRegistry::new();
        registry.upsert_peer("p1", Some("A".into()), 10).await;
        registry.upsert_peer("p2", Some("B".into()), 20).await;
        registry.upsert_peer("p3", Some("C".into()), 30).await;

        let peers = registry.list_peers().await;
        assert_eq!(peers.len(), 3);

        // Verify all peers are present (order not guaranteed with HashMap)
        let ids: Vec<String> = peers.iter().map(|p| p.node_id.clone()).collect();
        assert!(ids.contains(&"p1".to_string()));
        assert!(ids.contains(&"p2".to_string()));
        assert!(ids.contains(&"p3".to_string()));
    }

    // ── online_peers: mixed online/offline ───────────────────────────

    #[tokio::test]
    async fn test_online_peers_mixed() {
        let registry = PeerRegistry::new();
        registry.upsert_peer("p1", None, 0).await;
        registry.upsert_peer("p2", None, 0).await;
        registry.upsert_peer("p3", None, 0).await;

        registry.mark_offline("p2").await;

        let online = registry.online_peers().await;
        assert_eq!(online.len(), 2);
        let ids: Vec<String> = online.iter().map(|p| p.node_id.clone()).collect();
        assert!(ids.contains(&"p1".to_string()));
        assert!(ids.contains(&"p3".to_string()));
        assert!(!ids.contains(&"p2".to_string()));
    }

    // ── online_peers: all offline ────────────────────────────────────

    #[tokio::test]
    async fn test_online_peers_all_offline() {
        let registry = PeerRegistry::new();
        registry.upsert_peer("p1", None, 0).await;
        registry.upsert_peer("p2", None, 0).await;
        registry.mark_offline("p1").await;
        registry.mark_offline("p2").await;

        assert!(registry.online_peers().await.is_empty());
    }

    // ── peer_count ───────────────────────────────────────────────────

    #[tokio::test]
    async fn test_peer_count_incremental() {
        let registry = PeerRegistry::new();
        assert_eq!(registry.peer_count().await, 0);
        registry.upsert_peer("p1", None, 0).await;
        assert_eq!(registry.peer_count().await, 1);
        registry.upsert_peer("p2", None, 0).await;
        assert_eq!(registry.peer_count().await, 2);
        // Upserting same peer doesn't increase count
        registry.upsert_peer("p1", None, 5).await;
        assert_eq!(registry.peer_count().await, 2);
    }

    // ── PeerInfo serde roundtrip ─────────────────────────────────────

    #[test]
    fn test_peer_info_serde_roundtrip() {
        let info = PeerInfo {
            node_id: "abc123".to_string(),
            name: Some("Test Peer".to_string()),
            version: Some("0.1.0".to_string()),
            track_count: 42,
            last_seen: chrono::Utc::now(),
            is_online: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        let decoded: PeerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.node_id, "abc123");
        assert_eq!(decoded.name, Some("Test Peer".to_string()));
        assert_eq!(decoded.track_count, 42);
        assert!(decoded.is_online);
    }

    #[test]
    fn test_peer_info_serde_no_name() {
        let info = PeerInfo {
            node_id: "x".to_string(),
            name: None,
            version: None,
            track_count: 0,
            last_seen: chrono::Utc::now(),
            is_online: false,
        };
        let json = serde_json::to_string(&info).unwrap();
        let decoded: PeerInfo = serde_json::from_str(&json).unwrap();
        assert!(decoded.name.is_none());
        assert!(!decoded.is_online);
    }

    #[test]
    fn test_peer_info_clone() {
        let info = PeerInfo {
            node_id: "id".to_string(),
            name: Some("name".to_string()),
            version: Some("0.1.0".to_string()),
            track_count: 10,
            last_seen: chrono::Utc::now(),
            is_online: true,
        };
        let cloned = info.clone();
        assert_eq!(info.node_id, cloned.node_id);
        assert_eq!(info.name, cloned.name);
        assert_eq!(info.track_count, cloned.track_count);
    }

    #[test]
    fn test_peer_info_debug() {
        let info = PeerInfo {
            node_id: "dbg".to_string(),
            name: None,
            version: None,
            track_count: 0,
            last_seen: chrono::Utc::now(),
            is_online: false,
        };
        let debug = format!("{:?}", info);
        assert!(debug.contains("PeerInfo"));
        assert!(debug.contains("dbg"));
    }

    // ── Concurrent access ────────────────────────────────────────────

    #[tokio::test]
    async fn test_concurrent_upsert() {
        let registry = Arc::new(PeerRegistry::new());
        let mut handles = Vec::new();

        for i in 0..10 {
            let reg = Arc::clone(&registry);
            handles.push(tokio::spawn(async move {
                reg.upsert_peer(&format!("peer{i}"), Some(format!("P{i}")), i as u64)
                    .await;
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(registry.peer_count().await, 10);
    }

    #[tokio::test]
    async fn test_concurrent_read_write() {
        let registry = Arc::new(PeerRegistry::new());
        for i in 0..5 {
            registry.upsert_peer(&format!("p{i}"), None, i as u64).await;
        }

        let reg1 = Arc::clone(&registry);
        let reg2 = Arc::clone(&registry);

        let writer = tokio::spawn(async move {
            for i in 5..10 {
                reg1.upsert_peer(&format!("p{i}"), None, i as u64).await;
            }
        });

        let reader = tokio::spawn(async move {
            for _ in 0..10 {
                let _ = reg2.list_peers().await;
                let _ = reg2.online_peers().await;
                let _ = reg2.peer_count().await;
            }
        });

        writer.await.unwrap();
        reader.await.unwrap();

        assert_eq!(registry.peer_count().await, 10);
    }
}
