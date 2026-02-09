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
        let mut peers = self.peers.write().await;
        let info = peers
            .entry(node_id.to_string())
            .or_insert_with(|| PeerInfo {
                node_id: node_id.to_string(),
                name: None,
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
        }) => {
            registry.upsert_peer(&node_id, None, track_count).await;
            let info = registry.get_peer(&node_id).await.expect("just inserted");
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
            }) => {
                registry
                    .upsert_peer(&peer.node_id, peer.name.clone(), track_count)
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
}
