//! Library synchronization status & management.
//!
//! Provides per-peer library sync status comparison (local tracks vs. what
//! they've announced), and background full-sync tasks with progress tracking.

use std::sync::Arc;

use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};
use serde::Serialize;
use tokio::sync::Mutex;
use tracing::{info, warn};

use soundtime_db::entities::{remote_track, track};

use crate::discovery::PeerInfo;
use crate::node::P2pNode;

// ─── Sync status types ─────────────────────────────────────────────

/// Per-peer library synchronization status.
#[derive(Debug, Clone, Serialize)]
pub struct PeerSyncStatus {
    /// Peer's EndpointId
    pub node_id: String,
    /// Peer display name
    pub name: Option<String>,
    /// Software version
    pub version: Option<String>,
    /// Whether the peer is currently reachable
    pub is_online: bool,
    /// Number of tracks announced by this peer (from PeerInfo registry)
    pub peer_announced_tracks: u64,
    /// Number of remote tracks we've actually cataloged from this peer
    pub local_remote_tracks: u64,
    /// Number of remote tracks marked available from this peer
    pub available_tracks: u64,
    /// Number of local tracks with content_hash (our library size)
    pub our_track_count: u64,
    /// Sync ratio: local_remote_tracks / peer_announced_tracks (0.0-1.0)
    pub sync_ratio: f64,
    /// Human readable sync state
    pub sync_state: SyncState,
    /// Last time we heard from this peer
    pub last_seen: String,
}

/// High-level sync state for display.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    /// All announced tracks are cataloged
    Synced,
    /// Some tracks missing
    Partial,
    /// No tracks cataloged from this peer
    NotSynced,
    /// Peer is offline, can't determine
    Offline,
    /// Peer has 0 tracks (nothing to sync)
    Empty,
}

/// Overview of the full library sync status across all peers.
#[derive(Debug, Clone, Serialize)]
pub struct LibrarySyncOverview {
    /// Our local uploaded track count
    pub local_track_count: u64,
    /// Total peers known
    pub total_peers: usize,
    /// Peers fully synced
    pub synced_peers: usize,
    /// Peers partially synced
    pub partial_peers: usize,
    /// Peers not synced at all
    pub not_synced_peers: usize,
    /// Per-peer status
    pub peers: Vec<PeerSyncStatus>,
}

// ─── Sync task progress tracking ────────────────────────────────────

/// Progress of a background library sync task.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status")]
pub enum LibrarySyncTaskStatus {
    /// No sync task running
    #[serde(rename = "idle")]
    Idle,
    /// A sync task is currently running
    #[serde(rename = "running")]
    Running {
        peer_id: String,
        progress: SyncProgress,
    },
    /// Sync completed
    #[serde(rename = "completed")]
    Completed { result: SyncResult },
    /// Sync failed
    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncProgress {
    /// Tracks processed so far
    pub processed: u64,
    /// Total tracks to sync (if known)
    pub total: Option<u64>,
    /// Current phase description
    pub phase: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncResult {
    pub peer_id: String,
    pub tracks_synced: u64,
    pub tracks_already_known: u64,
    pub errors: u64,
    pub duration_secs: f64,
}

/// Shared handle for the sync task tracker.
pub type SyncTaskHandle = Arc<Mutex<LibrarySyncTaskStatus>>;

/// Create a new sync task tracker.
pub fn new_sync_tracker() -> SyncTaskHandle {
    Arc::new(Mutex::new(LibrarySyncTaskStatus::Idle))
}

// ─── Query functions ────────────────────────────────────────────────

/// Get the library sync overview for all known peers.
pub async fn get_library_sync_overview(
    node: &Arc<P2pNode>,
    db: &DatabaseConnection,
) -> LibrarySyncOverview {
    let peers = node.registry().list_peers().await;

    // Count our local uploaded tracks (tracks with content_hash, not p2p)
    let local_track_count = track::Entity::find()
        .filter(track::Column::ContentHash.is_not_null())
        .filter(track::Column::FilePath.not_like("p2p://%"))
        .count(db)
        .await
        .unwrap_or(0);

    let mut peer_statuses = Vec::with_capacity(peers.len());
    let mut synced = 0usize;
    let mut partial = 0usize;
    let mut not_synced = 0usize;

    for peer in &peers {
        let status = get_peer_sync_status(peer, local_track_count, db).await;
        match status.sync_state {
            SyncState::Synced => synced += 1,
            SyncState::Partial => partial += 1,
            SyncState::NotSynced => not_synced += 1,
            _ => {}
        }
        peer_statuses.push(status);
    }

    LibrarySyncOverview {
        local_track_count,
        total_peers: peers.len(),
        synced_peers: synced,
        partial_peers: partial,
        not_synced_peers: not_synced,
        peers: peer_statuses,
    }
}

/// Get sync status for a single peer.
async fn get_peer_sync_status(
    peer: &PeerInfo,
    our_track_count: u64,
    db: &DatabaseConnection,
) -> PeerSyncStatus {
    // Count remote tracks cataloged from this peer
    // The instance_domain for P2P peers is stored as their node_id
    let local_remote_tracks = remote_track::Entity::find()
        .filter(remote_track::Column::InstanceDomain.eq(&peer.node_id))
        .count(db)
        .await
        .unwrap_or(0);

    let available_tracks = remote_track::Entity::find()
        .filter(remote_track::Column::InstanceDomain.eq(&peer.node_id))
        .filter(remote_track::Column::IsAvailable.eq(true))
        .count(db)
        .await
        .unwrap_or(0);

    let announced = peer.track_count;
    let sync_ratio = if announced > 0 {
        (local_remote_tracks as f64) / (announced as f64)
    } else {
        0.0
    };

    let sync_state = if !peer.is_online {
        SyncState::Offline
    } else if announced == 0 {
        SyncState::Empty
    } else if local_remote_tracks >= announced {
        SyncState::Synced
    } else if local_remote_tracks > 0 {
        SyncState::Partial
    } else {
        SyncState::NotSynced
    };

    PeerSyncStatus {
        node_id: peer.node_id.clone(),
        name: peer.name.clone(),
        version: peer.version.clone(),
        is_online: peer.is_online,
        peer_announced_tracks: announced,
        local_remote_tracks,
        available_tracks,
        our_track_count,
        sync_ratio: (sync_ratio * 100.0).round() / 100.0, // 2 decimal places
        sync_state,
        last_seen: peer.last_seen.to_rfc3339(),
    }
}

// ─── Force re-sync ──────────────────────────────────────────────────

/// Trigger a full library re-sync with a specific peer in the background.
/// This sends a CatalogSync request and also requests the peer's full catalog.
pub fn spawn_library_resync(node: Arc<P2pNode>, peer_node_id: String, tracker: SyncTaskHandle) {
    tokio::spawn(async move {
        let start = std::time::Instant::now();

        // Set running status
        {
            let mut status = tracker.lock().await;
            *status = LibrarySyncTaskStatus::Running {
                peer_id: peer_node_id.clone(),
                progress: SyncProgress {
                    processed: 0,
                    total: None,
                    phase: "Connecting to peer...".to_string(),
                },
            };
        }

        let nid: iroh::EndpointId = match peer_node_id.parse() {
            Ok(id) => id,
            Err(_) => {
                let mut status = tracker.lock().await;
                *status = LibrarySyncTaskStatus::Error {
                    message: format!("Invalid node ID: {peer_node_id}"),
                };
                return;
            }
        };

        // Phase 1: Ping peer to verify connectivity
        {
            let mut status = tracker.lock().await;
            *status = LibrarySyncTaskStatus::Running {
                peer_id: peer_node_id.clone(),
                progress: SyncProgress {
                    processed: 0,
                    total: None,
                    phase: "Pinging peer...".to_string(),
                },
            };
        }

        let peer_addr = iroh::EndpointAddr::new(nid);
        match node.ping_peer(peer_addr).await {
            Ok(crate::node::P2pMessage::Pong {
                node_id: ref nid_str,
                track_count,
                version,
            }) => {
                node.registry()
                    .upsert_peer_versioned(nid_str, None, track_count, version)
                    .await;
                info!(peer = %peer_node_id, %track_count, "peer responded to ping — starting sync");

                // Update total
                let mut status = tracker.lock().await;
                *status = LibrarySyncTaskStatus::Running {
                    peer_id: peer_node_id.clone(),
                    progress: SyncProgress {
                        processed: 0,
                        total: Some(track_count),
                        phase: "Sending our catalog...".to_string(),
                    },
                };
            }
            Ok(_) => {
                warn!(peer = %peer_node_id, "unexpected response from peer");
            }
            Err(e) => {
                let mut status = tracker.lock().await;
                *status = LibrarySyncTaskStatus::Error {
                    message: format!("Failed to reach peer: {e}"),
                };
                return;
            }
        }

        // Phase 2: Send our full catalog to the peer
        {
            let mut status = tracker.lock().await;
            *status = LibrarySyncTaskStatus::Running {
                peer_id: peer_node_id.clone(),
                progress: SyncProgress {
                    processed: 0,
                    total: None,
                    phase: "Sending our catalog to peer...".to_string(),
                },
            };
        }
        node.announce_all_tracks_to_peer(nid).await;

        // Phase 3: Request peer's full catalog (peer exchange + catalog sync)
        {
            let mut status = tracker.lock().await;
            *status = LibrarySyncTaskStatus::Running {
                peer_id: peer_node_id.clone(),
                progress: SyncProgress {
                    processed: 1,
                    total: Some(3),
                    phase: "Requesting peer's catalog...".to_string(),
                },
            };
        }

        // Discover via peer (PEX + catalog)
        node.discover_via_peer(nid).await;

        // Phase 4: Exchange bloom filters for search
        {
            let mut status = tracker.lock().await;
            *status = LibrarySyncTaskStatus::Running {
                peer_id: peer_node_id.clone(),
                progress: SyncProgress {
                    processed: 2,
                    total: Some(3),
                    phase: "Exchanging search indexes...".to_string(),
                },
            };
        }
        node.broadcast_bloom_filter().await;

        // Phase 5: Done — incremental sync to pull any missing data
        {
            let mut status = tracker.lock().await;
            *status = LibrarySyncTaskStatus::Running {
                peer_id: peer_node_id.clone(),
                progress: SyncProgress {
                    processed: 3,
                    total: Some(3),
                    phase: "Finalizing incremental sync...".to_string(),
                },
            };
        }
        node.incremental_sync_to_peer(nid, None).await;

        // Count results
        let db = node.db();
        let tracks_synced = remote_track::Entity::find()
            .filter(remote_track::Column::InstanceDomain.eq(&peer_node_id))
            .count(db)
            .await
            .unwrap_or(0);

        let elapsed = start.elapsed().as_secs_f64();
        info!(
            peer = %peer_node_id,
            tracks = tracks_synced,
            duration_secs = elapsed,
            "library re-sync completed"
        );

        // Set completed
        {
            let mut status = tracker.lock().await;
            *status = LibrarySyncTaskStatus::Completed {
                result: SyncResult {
                    peer_id: peer_node_id,
                    tracks_synced,
                    tracks_already_known: 0, // We don't track this granularity yet
                    errors: 0,
                    duration_secs: (elapsed * 100.0).round() / 100.0,
                },
            };
        }
    });
}
