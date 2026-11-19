//! P2P API routes — status, peer management, track sharing, distributed search, library sync.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::{ActiveModelTrait, EntityTrait, PaginatorTrait, Set};
use serde::{Deserialize, Serialize};
use soundtime_db::entities::remote_track;
use soundtime_db::AppState;
use soundtime_p2p::rendezvous::{Registration, RegistrationError, Rendezvous};
use soundtime_p2p::{
    get_library_sync_overview, spawn_library_resync, LibrarySyncOverview, LibrarySyncTaskStatus,
    SyncTaskHandle,
};
use soundtime_p2p::{P2pMessage, P2pNode, PeerInfo};
use std::sync::Arc;
use uuid::Uuid;

/// POST /api/p2p/rendezvous on instances that explicitly host a directory.
pub async fn rendezvous(
    Extension(directory): Extension<Arc<tokio::sync::Mutex<Rendezvous>>>,
    Json(registration): Json<Registration>,
) -> Result<Json<Vec<soundtime_p2p::NodeId>>, StatusCode> {
    directory
        .lock()
        .await
        .register(registration, chrono::Utc::now().timestamp())
        .map(Json)
        .map_err(|err| match err {
            RegistrationError::Invalid => StatusCode::BAD_REQUEST,
            RegistrationError::Full => StatusCode::SERVICE_UNAVAILABLE,
        })
}

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
    pub dht_discovery_enabled: bool,
    pub rendezvous_configured: bool,
}

#[derive(Deserialize)]
pub struct AddPeerRequest {
    /// iroh NodeId (public key) of the peer to add
    pub node_id: String,
    /// Optional numeric transport addresses supplied by the administrator.
    #[serde(default)]
    pub direct_addresses: Vec<std::net::SocketAddr>,
    #[serde(default)]
    pub follow_catalog: Option<bool>,
}

#[derive(Deserialize)]
pub struct SubscriptionRequest {
    pub followed: bool,
}

#[derive(Serialize)]
pub struct MessageResponse {
    pub message: String,
}

// ── Handlers ────────────────────────────────────────────────────

/// GET /api/p2p/status — P2P node status (public, gated by instance privacy)
pub async fn p2p_status(State(state): State<Arc<AppState>>) -> Json<P2pStatus> {
    let Some(node) = get_p2p_node(&state) else {
        return Json(P2pStatus {
            enabled: false,
            node_id: None,
            relay_url: None,
            relay_connected: false,
            direct_addresses: 0,
            peer_count: 0,
            online_peer_count: 0,
            dht_discovery_enabled: false,
            rendezvous_configured: false,
        });
    };

    let relay_url = node.relay_url();
    let relay_connected = relay_url.is_some();
    let direct_addresses = node.direct_addresses_count();

    Json(P2pStatus {
        enabled: true,
        node_id: Some(node.node_id().to_string()),
        relay_url,
        relay_connected,
        direct_addresses,
        peer_count: node.registry().peer_count().await,
        online_peer_count: node.registry().online_peers().await.len(),
        dht_discovery_enabled: node.dht_discovery_enabled(),
        rendezvous_configured: node.rendezvous_configured(),
    })
}

/// Transport diagnostics include private addresses and are only visible to admins.
pub async fn transport_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<soundtime_p2p::node::TransportStats>, StatusCode> {
    let node = get_p2p_node(&state).ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(node.transport_stats()))
}

#[derive(Serialize)]
pub struct MirrorProviders {
    providers: Vec<String>,
}

/// Inspect usable, signed mirror leases for the current version of a local track ID.
pub async fn track_mirrors(
    State(state): State<Arc<AppState>>,
    Path(track): Path<Uuid>,
) -> Result<Json<MirrorProviders>, StatusCode> {
    use sea_orm::{ConnectionTrait, DbBackend, Statement};
    let rows=state.db.query_all(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT DISTINCT l.provider FROM federation_mirror_leases l JOIN track_publications p ON p.track_id=l.track_id AND p.revision=l.revision AND p.state='active'
         JOIN federation_track_providers s ON s.track_id=p.track_id AND s.provider=l.provider AND s.allowed AND s.followed AND s.online
         WHERE l.track_id=$1 AND l.active AND l.expires_at>now() ORDER BY l.provider LIMIT 32",
         [track.into()])).await.map_err(|_|StatusCode::SERVICE_UNAVAILABLE)?;
    let providers = rows
        .into_iter()
        .map(|r| r.try_get("", "provider"))
        .collect::<Result<_, _>>()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(MirrorProviders { providers }))
}

/// GET /api/admin/p2p/peers — list known peers (admin only)
pub async fn list_peers(State(state): State<Arc<AppState>>) -> Json<Vec<PeerInfo>> {
    let Some(node) = get_p2p_node(&state) else {
        return Json(vec![]);
    };

    let peers = node.registry().list_peers().await;
    Json(peers)
}

/// POST /api/admin/p2p/peers — add a peer by NodeId (admin only)
pub async fn add_peer(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AddPeerRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), (StatusCode, Json<MessageResponse>)> {
    let Some(node) = get_p2p_node(&state) else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(MessageResponse {
                message: "P2P node is not enabled".to_string(),
            }),
        ));
    };

    let node_id: soundtime_p2p::NodeId = payload.node_id.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(MessageResponse {
                message: "Invalid node ID format".to_string(),
            }),
        )
    })?;

    if payload.direct_addresses.len() > 16
        || payload.direct_addresses.iter().any(|addr| {
            addr.port() == 0
                || addr.ip().is_unspecified()
                || addr.ip().is_multicast()
                || matches!(addr.ip(), std::net::IpAddr::V4(ip) if ip.is_broadcast())
        })
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(MessageResponse {
                message: "Expected at most 16 unicast addresses with nonzero ports".into(),
            }),
        ));
    }
    if payload.follow_catalog == Some(false) && !node.explicit_catalog() {
        return Err((
            StatusCode::CONFLICT,
            Json(MessageResponse {
                message: "Presence-only discovery requires explicit catalogue mode".into(),
            }),
        ));
    }
    let follow = payload.follow_catalog.unwrap_or(true);
    if node.explicit_catalog() {
        node.set_followed(node_id, follow)
            .await
            .map_err(peer_policy_error)?;
    }
    let mut peer_addr = soundtime_p2p::NodeAddr::new(node_id);
    for address in payload.direct_addresses {
        peer_addr = peer_addr.with_ip_addr(address);
    }
    let started = chrono::Utc::now();
    match node.ping_peer(peer_addr).await {
        Ok(P2pMessage::Pong { track_count, .. }) => {
            let preparing = if node.explicit_catalog() && follow {
                match node.request_catalog_from_peer(node_id).await {
                    Ok(()) => false,
                    Err(soundtime_p2p::P2pError::CatalogPreparing) => true,
                    Err(error) => return Err(peer_policy_error(error)),
                }
            } else {
                false
            };
            // Trigger peer exchange in background to discover wider network
            let p2p_clone = Arc::clone(&node);
            tokio::spawn(async move {
                p2p_clone.discover_via_peer(node_id).await;
            });
            if preparing {
                return Ok(catalog_preparing());
            }
            Ok((
                StatusCode::OK,
                Json(MessageResponse {
                    message: format!(
                        "peer {} added and responded to ping ({} tracks)",
                        payload.node_id, track_count
                    ),
                }),
            ))
        }
        Ok(_) => {
            // Got a response but not a Pong — register anyway
            node.registry().upsert_peer(&payload.node_id, None, 0).await;
            Ok((
                StatusCode::OK,
                Json(MessageResponse {
                    message: format!("peer {} added (unexpected response type)", payload.node_id),
                }),
            ))
        }
        Err(e) => {
            if matches!(
                e,
                soundtime_p2p::P2pError::Connection(_)
                    | soundtime_p2p::P2pError::Endpoint(_)
                    | soundtime_p2p::P2pError::Io(_)
            ) {
                node.registry()
                    .failed_since(&payload.node_id, started, true)
                    .await;
            }
            tracing::warn!(peer = %payload.node_id, "ping failed: {e}");
            Ok((
                StatusCode::OK,
                Json(MessageResponse {
                    message: format!("peer {} added but ping failed: {e}", payload.node_id),
                }),
            ))
        }
    }
}

fn catalog_preparing() -> (StatusCode, Json<MessageResponse>) {
    (StatusCode::ACCEPTED, Json(MessageResponse {
        message: "Catalogue followed. The remote journal is preparing. Synchronization will resume automatically.".into(),
    }))
}

fn peer_policy_error(error: soundtime_p2p::P2pError) -> (StatusCode, Json<MessageResponse>) {
    let code = match error {
        soundtime_p2p::P2pError::PeerBlocked(_) => StatusCode::FORBIDDEN,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    (
        code,
        Json(MessageResponse {
            message: error.to_string(),
        }),
    )
}

/// Persist the local subscription before notifying the remote provider.
pub async fn update_subscription(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(payload): Json<SubscriptionRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), (StatusCode, Json<MessageResponse>)> {
    let node = get_p2p_node(&state)
        .filter(|n| n.explicit_catalog())
        .ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                Json(MessageResponse {
                    message: "Explicit catalogue mode is disabled".into(),
                }),
            )
        })?;
    let id = id.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(MessageResponse {
                message: "Invalid node ID".into(),
            }),
        )
    })?;
    let notified = match node.update_subscription(id, payload.followed).await {
        Ok(notified) => notified,
        Err(soundtime_p2p::P2pError::CatalogPreparing) => return Ok(catalog_preparing()),
        Err(error) => return Err(peer_policy_error(error)),
    };
    Ok((
        StatusCode::OK,
        Json(MessageResponse {
            message: format!(
                "Catalogue {}{}",
                if payload.followed {
                    "followed"
                } else {
                    "unfollowed"
                },
                if notified {
                    ""
                } else {
                    " locally (peer not notified)"
                }
            ),
        }),
    ))
}

/// POST /api/admin/p2p/peers/{node_id}/ping — ping a specific peer (admin only)
pub async fn ping_peer(
    State(state): State<Arc<AppState>>,
    Path(peer_node_id): Path<String>,
) -> Result<Json<MessageResponse>, (StatusCode, Json<MessageResponse>)> {
    let Some(node) = get_p2p_node(&state) else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(MessageResponse {
                message: "P2P node is not enabled".to_string(),
            }),
        ));
    };

    let node_id: soundtime_p2p::NodeId = peer_node_id.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(MessageResponse {
                message: "Invalid node ID format".to_string(),
            }),
        )
    })?;

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
) -> Result<Json<MessageResponse>, (StatusCode, Json<MessageResponse>)> {
    if let Some(node) = get_p2p_node(&state) {
        if node.explicit_catalog() {
            let nid = peer_node_id.parse().map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(MessageResponse {
                        message: "Invalid node ID".into(),
                    }),
                )
            })?;
            node.update_subscription(nid, false)
                .await
                .map_err(peer_policy_error)?;
        }
        node.registry().remove_peer(&peer_node_id).await;
    }
    Ok(Json(MessageResponse {
        message: format!("peer {peer_node_id} removed"),
    }))
}

// ── Network graph types ─────────────────────────────────────────

#[derive(Serialize)]
pub struct NetworkGraphNode {
    pub id: String,
    /// "self", "peer", "relay"
    pub node_type: String,
    pub label: String,
    pub online: bool,
    /// Number of tracks on this node (for peers: from Pong data; for self: from DB)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_count: Option<u64>,
    /// Software version (for peers: from Pong data)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
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
pub async fn network_graph(State(state): State<Arc<AppState>>) -> Json<NetworkGraph> {
    let Some(node) = get_p2p_node(&state) else {
        return Json(NetworkGraph {
            nodes: vec![],
            links: vec![],
        });
    };

    let mut nodes = Vec::new();
    let mut links = Vec::new();

    let node_id = node.node_id().to_string();
    let short_id = if node_id.len() > 8 {
        &node_id[..8]
    } else {
        &node_id
    };

    // Self node — get local track count
    let self_track_count = {
        use soundtime_db::entities::track;
        track::Entity::find().count(&state.db).await.unwrap_or(0)
    };

    // Self node (always present)
    nodes.push(NetworkGraphNode {
        id: node_id.clone(),
        node_type: "self".to_string(),
        label: format!("Me ({short_id}…)"),
        online: true,
        track_count: Some(self_track_count),
        version: Some(soundtime_p2p::build_version().to_string()),
    });

    // Relay node
    if let Some(relay_url) = node.relay_url() {
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
            track_count: None,
            version: None,
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
            track_count: Some(peer.track_count),
            version: peer.version.clone(),
        });
        links.push(NetworkGraphLink {
            source: node_id.clone(),
            target: peer.node_id.clone(),
            link_type: "peer".to_string(),
        });
    }

    Json(NetworkGraph { nodes, links })
}

// ── Distributed search ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct NetworkSearchQuery {
    pub q: String,
    pub limit: Option<u32>,
}

#[derive(Serialize)]
pub struct NetworkSearchResponse {
    pub results: Vec<super::search::RemoteSearchHit>,
    pub total: usize,
}

/// GET /api/p2p/search?q=...&limit=... — distributed search across the P2P network.
/// Queries peers whose Bloom filter indicates they might have matching content.
pub async fn network_search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<NetworkSearchQuery>,
) -> Result<Json<NetworkSearchResponse>, (StatusCode, Json<MessageResponse>)> {
    let Some(node) = get_p2p_node(&state) else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(MessageResponse {
                message: "P2P node is not enabled".to_string(),
            }),
        ));
    };

    let query = params.q.trim();
    if query.is_empty() {
        return Ok(Json(NetworkSearchResponse {
            results: vec![],
            total: 0,
        }));
    }

    let limit = params.limit.unwrap_or(20).min(100);
    let results = super::search::network_results(&state, node, query, limit)
        .await
        .map_err(|(code, message)| (code, Json(MessageResponse { message })))?;
    let total = results.len();

    Ok(Json(NetworkSearchResponse { results, total }))
}

// ── Library Sync ────────────────────────────────────────────────

/// GET /api/admin/p2p/library-sync — library sync overview for all peers (admin only)
pub async fn library_sync_overview(
    State(state): State<Arc<AppState>>,
) -> Json<LibrarySyncOverview> {
    let Some(node) = get_p2p_node(&state) else {
        return Json(LibrarySyncOverview {
            local_track_count: 0,
            total_peers: 0,
            synced_peers: 0,
            partial_peers: 0,
            not_synced_peers: 0,
            peers: vec![],
        });
    };

    let overview = get_library_sync_overview(&node, &state.db).await;
    Json(overview)
}

/// POST /api/admin/p2p/library-sync/:node_id — force a full re-sync with a peer (admin only)
pub async fn trigger_library_resync(
    State(state): State<Arc<AppState>>,
    Extension(tracker): Extension<SyncTaskHandle>,
    Path(peer_node_id): Path<String>,
) -> Result<Json<MessageResponse>, (StatusCode, Json<MessageResponse>)> {
    let Some(node) = get_p2p_node(&state) else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(MessageResponse {
                message: "P2P node is not enabled".to_string(),
            }),
        ));
    };

    // Check if a sync is already running
    {
        let status = tracker.lock().await;
        if let LibrarySyncTaskStatus::Running { .. } = &*status {
            return Err((
                StatusCode::CONFLICT,
                Json(MessageResponse {
                    message: "A library sync is already in progress".to_string(),
                }),
            ));
        }
    }

    // Validate peer exists
    if node.registry().get_peer(&peer_node_id).await.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(MessageResponse {
                message: format!("Peer {peer_node_id} not found"),
            }),
        ));
    }

    spawn_library_resync(node, peer_node_id.clone(), tracker);

    Ok(Json(MessageResponse {
        message: format!("Library re-sync started with peer {peer_node_id}"),
    }))
}

/// GET /api/admin/p2p/library-sync/task-status — poll the background sync task status
pub async fn library_sync_task_status(
    Extension(tracker): Extension<SyncTaskHandle>,
) -> Json<LibrarySyncTaskStatus> {
    let status = tracker.lock().await;
    Json(status.clone())
}

/// POST /api/admin/p2p/library-sync/task-dismiss — reset task status to idle
pub async fn library_sync_task_dismiss(
    Extension(tracker): Extension<SyncTaskHandle>,
) -> Json<MessageResponse> {
    let mut status = tracker.lock().await;
    *status = LibrarySyncTaskStatus::Idle;
    Json(MessageResponse {
        message: "Task status reset".to_string(),
    })
}

// ── Remote track dereference / rereference ───────────────────────

/// PATCH /api/admin/p2p/tracks/{id}/dereference — mark a remote track as unavailable (admin only)
pub async fn dereference_remote_track(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<MessageResponse>, (StatusCode, Json<MessageResponse>)> {
    let track = remote_track::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to query remote track: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MessageResponse {
                    message: "Database error".to_string(),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(MessageResponse {
                    message: format!("Remote track {id} not found"),
                }),
            )
        })?;

    let mut update: remote_track::ActiveModel = track.into();
    update.is_available = Set(false);
    update.update(&state.db).await.map_err(|e| {
        tracing::error!("Failed to dereference remote track {id}: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MessageResponse {
                message: "Failed to update remote track".to_string(),
            }),
        )
    })?;

    Ok(Json(MessageResponse {
        message: format!("Remote track {id} dereferenced (marked unavailable)"),
    }))
}

/// PATCH /api/admin/p2p/tracks/{id}/rereference — mark a remote track as available (admin only)
pub async fn rereference_remote_track(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<MessageResponse>, (StatusCode, Json<MessageResponse>)> {
    let track = remote_track::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to query remote track: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MessageResponse {
                    message: "Database error".to_string(),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(MessageResponse {
                    message: format!("Remote track {id} not found"),
                }),
            )
        })?;

    let mut update: remote_track::ActiveModel = track.into();
    update.is_available = Set(true);
    update.update(&state.db).await.map_err(|e| {
        tracing::error!("Failed to rereference remote track {id}: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(MessageResponse {
                message: "Failed to update remote track".to_string(),
            }),
        )
    })?;

    Ok(Json(MessageResponse {
        message: format!("Remote track {id} rereferenced (marked available)"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1. P2pStatus serialization (disabled)
    #[test]
    fn test_serialize_p2p_status_disabled() {
        let status = P2pStatus {
            enabled: false,
            node_id: None,
            relay_url: None,
            relay_connected: false,
            direct_addresses: 0,
            peer_count: 0,
            online_peer_count: 0,
            dht_discovery_enabled: false,
            rendezvous_configured: false,
        };
        let val = serde_json::to_value(&status).unwrap();
        assert_eq!(val["enabled"], false);
        assert!(val["node_id"].is_null());
        assert_eq!(val["dht_discovery_enabled"], false);
    }

    // 2. P2pStatus serialization (enabled)
    #[test]
    fn test_serialize_p2p_status_enabled() {
        let status = P2pStatus {
            enabled: true,
            node_id: Some("abc123".to_string()),
            relay_url: Some("https://relay.example.com".to_string()),
            relay_connected: true,
            direct_addresses: 2,
            peer_count: 5,
            online_peer_count: 3,
            dht_discovery_enabled: true,
            rendezvous_configured: true,
        };
        let val = serde_json::to_value(&status).unwrap();
        assert_eq!(val["enabled"], true);
        assert_eq!(val["node_id"], "abc123");
        assert_eq!(val["peer_count"], 5);
        assert_eq!(val["dht_discovery_enabled"], true);
    }

    // 3. AddPeerRequest deserialization
    #[test]
    fn test_deserialize_add_peer_request() {
        let json = r#"{"node_id":"abc123def456"}"#;
        let req: AddPeerRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.node_id, "abc123def456");
        assert!(req.direct_addresses.is_empty());
        let with_ip =
            r#"{"node_id":"abc123def456","direct_addresses":["127.0.0.1:11204","[::1]:11204"]}"#;
        assert_eq!(
            serde_json::from_str::<AddPeerRequest>(with_ip)
                .unwrap()
                .direct_addresses
                .len(),
            2
        );
    }

    // 4. MessageResponse serialization
    #[test]
    fn test_serialize_message_response() {
        let resp = MessageResponse {
            message: "peer added".to_string(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["message"], "peer added");
    }

    // 5. NetworkGraphNode serialization
    #[test]
    fn test_serialize_network_graph_node() {
        let node = NetworkGraphNode {
            id: "node123".to_string(),
            node_type: "peer".to_string(),
            label: "Peer (node123)".to_string(),
            online: true,
            track_count: Some(42),
            version: Some("0.1.0".to_string()),
        };
        let val = serde_json::to_value(&node).unwrap();
        assert_eq!(val["node_type"], "peer");
        assert_eq!(val["track_count"], 42);
    }

    // 6. NetworkGraphNode with skip_serializing_if None
    #[test]
    fn test_serialize_network_graph_node_no_optional() {
        let node = NetworkGraphNode {
            id: "relay1".to_string(),
            node_type: "relay".to_string(),
            label: "Relay".to_string(),
            online: true,
            track_count: None,
            version: None,
        };
        let val = serde_json::to_value(&node).unwrap();
        // track_count and version have skip_serializing_if = "Option::is_none"
        assert!(val.get("track_count").is_none());
        assert!(val.get("version").is_none());
    }

    // 7. NetworkGraphLink serialization
    #[test]
    fn test_serialize_network_graph_link() {
        let link = NetworkGraphLink {
            source: "node1".to_string(),
            target: "node2".to_string(),
            link_type: "peer".to_string(),
        };
        let val = serde_json::to_value(&link).unwrap();
        assert_eq!(val["source"], "node1");
        assert_eq!(val["link_type"], "peer");
    }

    // 8. NetworkGraph serialization (empty)
    #[test]
    fn test_serialize_network_graph_empty() {
        let graph = NetworkGraph {
            nodes: vec![],
            links: vec![],
        };
        let val = serde_json::to_value(&graph).unwrap();
        assert!(val["nodes"].as_array().unwrap().is_empty());
        assert!(val["links"].as_array().unwrap().is_empty());
    }

    // 9. NetworkSearchQuery deserialization
    #[test]
    fn test_deserialize_network_search_query() {
        let json = r#"{"q":"rock music","limit":50}"#;
        let query: NetworkSearchQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.q, "rock music");
        assert_eq!(query.limit, Some(50));
    }

    // 10. NetworkSearchQuery without limit
    #[test]
    fn test_deserialize_network_search_query_no_limit() {
        let json = r#"{"q":"jazz"}"#;
        let query: NetworkSearchQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.q, "jazz");
        assert_eq!(query.limit, None);
    }

    // 11. p2p_status returns disabled when no P2P node
    #[tokio::test]
    async fn test_p2p_status_disabled() {
        use axum::{body::Body, http::Request, routing::get, Router};
        use tower::ServiceExt;

        let state = Arc::new(AppState {
            db: sea_orm::DatabaseConnection::Disconnected,
            jwt_secret: "test".to_string(),
            domain: "localhost".to_string(),
            storage: Arc::new(soundtime_audio::AudioStorage::new("/tmp/test")),
            p2p: None,
            plugins: None,
            #[cfg(feature = "redis")]
            redis: None,
        });

        let app = Router::new()
            .route("/p2p/status", get(p2p_status))
            .with_state(state);

        let req = Request::builder()
            .method("GET")
            .uri("/p2p/status")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["enabled"], false);
        assert!(val["node_id"].is_null());
        assert_eq!(val["dht_discovery_enabled"], false);
    }
}
