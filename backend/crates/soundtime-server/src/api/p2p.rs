//! P2P API routes — status, peer management, track sharing.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use soundtime_db::AppState;
use soundtime_p2p::{P2pMessage, P2pNode, PeerInfo};
use std::sync::Arc;

/// Helper: extract `Arc<P2pNode>` from type-erased AppState field.
fn get_p2p_node(state: &AppState) -> Option<Arc<P2pNode>> {
    state
        .p2p
        .as_ref()
        .and_then(|any| any.clone().downcast::<P2pNode>().ok())
}

// ── Response types ──────────────────────────────────────────────

#[derive(Serialize)]
pub struct P2pStatus {
    pub enabled: bool,
    pub node_id: Option<String>,
    pub relay_url: Option<String>,
    pub relay_connected: bool,
    pub direct_addresses: usize,
    pub peer_count: usize,
    pub online_peer_count: usize,
}

#[derive(Serialize)]
pub struct PeerListResponse {
    pub peers: Vec<PeerInfo>,
}

#[derive(Deserialize)]
pub struct AddPeerRequest {
    /// iroh NodeId (public key) of the peer to add
    pub node_id: String,
}

#[derive(Serialize)]
pub struct MessageResponse {
    pub message: String,
}

// ── Handlers ────────────────────────────────────────────────────

/// GET /api/p2p/status — P2P node status (public, gated by instance privacy)
pub async fn p2p_status(
    State(state): State<Arc<AppState>>,
) -> Json<P2pStatus> {
    let Some(node) = get_p2p_node(&state) else {
        return Json(P2pStatus {
            enabled: false,
            node_id: None,
            relay_url: None,
            relay_connected: false,
            direct_addresses: 0,
            peer_count: 0,
            online_peer_count: 0,
        });
    };

    let relay_url = node.relay_url().await;
    let relay_connected = relay_url.is_some();
    let direct_addresses = node.direct_addresses_count().await;

    Json(P2pStatus {
        enabled: true,
        node_id: Some(node.node_id().to_string()),
        relay_url,
        relay_connected,
        direct_addresses,
        peer_count: node.registry().peer_count().await,
        online_peer_count: node.registry().online_peers().await.len(),
    })
}

/// GET /api/admin/p2p/peers — list known peers (admin only)
pub async fn list_peers(
    State(state): State<Arc<AppState>>,
) -> Json<PeerListResponse> {
    let Some(node) = get_p2p_node(&state) else {
        return Json(PeerListResponse { peers: vec![] });
    };

    let peers = node.registry().list_peers().await;
    Json(PeerListResponse { peers })
}

/// POST /api/admin/p2p/peers — add a peer by NodeId (admin only)
pub async fn add_peer(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AddPeerRequest>,
) -> Result<Json<MessageResponse>, (StatusCode, Json<MessageResponse>)> {
    let Some(node) = get_p2p_node(&state) else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, Json(MessageResponse {
            message: "P2P node is not enabled".to_string(),
        })));
    };

    let node_id: soundtime_p2p::NodeId = payload
        .node_id
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(MessageResponse {
            message: "Invalid node ID format".to_string(),
        })))?;

    let peer_addr = soundtime_p2p::NodeAddr::new(node_id);
    match node.ping_peer(peer_addr).await {
        Ok(P2pMessage::Pong { node_id: peer_nid, track_count }) => {
            node.registry().upsert_peer(&peer_nid, None, track_count).await;
            Ok(Json(MessageResponse {
                message: format!("peer {} added and responded to ping ({} tracks)", payload.node_id, track_count),
            }))
        }
        Ok(_) => {
            // Got a response but not a Pong — register anyway
            node.registry().upsert_peer(&payload.node_id, None, 0).await;
            Ok(Json(MessageResponse {
                message: format!("peer {} added (unexpected response type)", payload.node_id),
            }))
        }
        Err(e) => {
            // Register as offline peer
            node.registry().upsert_peer(&payload.node_id, None, 0).await;
            node.registry().mark_offline(&payload.node_id).await;
            tracing::warn!(peer = %payload.node_id, "ping failed: {e}");
            Ok(Json(MessageResponse {
                message: format!("peer {} added but ping failed: {e}", payload.node_id),
            }))
        }
    }
}

/// POST /api/admin/p2p/peers/{node_id}/ping — ping a specific peer (admin only)
pub async fn ping_peer(
    State(state): State<Arc<AppState>>,
    Path(peer_node_id): Path<String>,
) -> Result<Json<MessageResponse>, (StatusCode, Json<MessageResponse>)> {
    let Some(node) = get_p2p_node(&state) else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, Json(MessageResponse {
            message: "P2P node is not enabled".to_string(),
        })));
    };

    let node_id: soundtime_p2p::NodeId = peer_node_id
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(MessageResponse {
            message: "Invalid node ID format".to_string(),
        })))?;

    let peer_addr = soundtime_p2p::NodeAddr::new(node_id);
    match node.ping_peer(peer_addr).await {
        Ok(msg) => Ok(Json(MessageResponse {
            message: format!("pong received: {msg:?}"),
        })),
        Err(e) => Ok(Json(MessageResponse {
            message: format!("ping failed: {e}"),
        })),
    }
}

/// DELETE /api/admin/p2p/peers/{node_id} — remove a peer (admin only)
pub async fn remove_peer(
    State(state): State<Arc<AppState>>,
    Path(peer_node_id): Path<String>,
) -> Json<MessageResponse> {
    if let Some(node) = get_p2p_node(&state) {
        node.registry().remove_peer(&peer_node_id).await;
    }
    Json(MessageResponse {
        message: format!("peer {peer_node_id} removed"),
    })
}

// ── Network graph types ─────────────────────────────────────────

#[derive(Serialize)]
pub struct NetworkGraphNode {
    pub id: String,
    /// "self", "peer", "relay"
    pub node_type: String,
    pub label: String,
    pub online: bool,
}

#[derive(Serialize)]
pub struct NetworkGraphLink {
    pub source: String,
    pub target: String,
    /// "relay", "direct", "peer"
    pub link_type: String,
}

#[derive(Serialize)]
pub struct NetworkGraph {
    pub nodes: Vec<NetworkGraphNode>,
    pub links: Vec<NetworkGraphLink>,
}

/// GET /api/p2p/network-graph — P2P network topology for D3 visualization
pub async fn network_graph(
    State(state): State<Arc<AppState>>,
) -> Json<NetworkGraph> {
    let Some(node) = get_p2p_node(&state) else {
        return Json(NetworkGraph {
            nodes: vec![],
            links: vec![],
        });
    };

    let mut nodes = Vec::new();
    let mut links = Vec::new();

    let node_id = node.node_id().to_string();
    let short_id = if node_id.len() > 8 { &node_id[..8] } else { &node_id };

    // Self node (always present)
    nodes.push(NetworkGraphNode {
        id: node_id.clone(),
        node_type: "self".to_string(),
        label: format!("Me ({short_id}…)"),
        online: true,
    });

    // Relay node
    if let Some(relay_url) = node.relay_url().await {
        let relay_id = format!("relay:{relay_url}");
        let relay_host = relay_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        nodes.push(NetworkGraphNode {
            id: relay_id.clone(),
            node_type: "relay".to_string(),
            label: relay_host,
            online: true,
        });
        links.push(NetworkGraphLink {
            source: node_id.clone(),
            target: relay_id,
            link_type: "relay".to_string(),
        });
    }

    // Peers from registry
    let peers = node.registry().list_peers().await;
    for peer in &peers {
        let short_peer_id = if peer.node_id.len() > 8 {
            &peer.node_id[..8]
        } else {
            &peer.node_id
        };
        let label = peer
            .name
            .as_deref()
            .map(|n| format!("{n} ({short_peer_id}…)"))
            .unwrap_or_else(|| format!("Peer ({short_peer_id}…)"));

        nodes.push(NetworkGraphNode {
            id: peer.node_id.clone(),
            node_type: "peer".to_string(),
            label,
            online: peer.is_online,
        });
        links.push(NetworkGraphLink {
            source: node_id.clone(),
            target: peer.node_id.clone(),
            link_type: "peer".to_string(),
        });
    }

    Json(NetworkGraph { nodes, links })
}
