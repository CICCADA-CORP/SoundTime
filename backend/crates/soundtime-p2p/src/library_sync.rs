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

// ─── Helpers ─────────────────────────────────────────────────────────

/// Normalize a node ID for comparison with the `instance_domain` column.
///
/// `instance_domain` stores values like `"p2p://node_id_here"`. This helper
/// ensures a bare node ID is prefixed so that Sea-ORM equality filters match
/// correctly, while already-prefixed values pass through unchanged.
fn normalize_instance_domain(node_id: &str) -> String {
    if node_id.starts_with("p2p://") {
        node_id.to_string()
    } else {
        format!("p2p://{}", node_id)
    }
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
    // Count remote tracks cataloged from this peer.
    // instance_domain is stored as "p2p://<node_id>" so we must normalize.
    // TODO: For transitive tracks (originated on peer C, relayed via peer B),
    // instance_domain reflects the *original* uploader, not the relay peer.
    // A full fix would aggregate sync status across all relay paths; for now
    // we match on the direct peer's normalized domain.
    let domain = normalize_instance_domain(&peer.node_id);

    let local_remote_tracks = remote_track::Entity::find()
        .filter(remote_track::Column::InstanceDomain.eq(&domain))
        .count(db)
        .await
        .unwrap_or(0);

    let available_tracks = remote_track::Entity::find()
        .filter(remote_track::Column::InstanceDomain.eq(&domain))
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
///
/// Runs through five phases:
/// 1. **Ping** — verify peer connectivity and learn its track count.
/// 2. **Send catalog** — push our local tracks to the peer via `announce_all_tracks_to_peer`.
/// 3. **Request catalog** — send `RequestCatalog` to the peer, then poll the
///    `remote_track` table until all expected tracks arrive (or a 5-minute
///    timeout / 20-second stall is reached).
/// 4. **Bloom exchange** — broadcast updated search indexes.
/// 5. **Incremental sync** — send any remaining tracks the peer may have missed.
///
/// Progress is tracked via the shared `tracker` handle so the API can report
/// real-time status to the frontend.
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
        let expected_tracks: u64;
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
                expected_tracks = track_count;

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
                let mut status = tracker.lock().await;
                *status = LibrarySyncTaskStatus::Error {
                    message: "Unexpected response from peer".to_string(),
                };
                return;
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

        // Phase 3: Request peer's full catalog and wait for tracks to arrive
        {
            let mut status = tracker.lock().await;
            *status = LibrarySyncTaskStatus::Running {
                peer_id: peer_node_id.clone(),
                progress: SyncProgress {
                    processed: 0,
                    total: Some(expected_tracks),
                    phase: "Requesting peer's catalog...".to_string(),
                },
            };
        }

        // Also run PEX for peer discovery (non-blocking, just fire and forget)
        node.discover_via_peer(nid).await;

        // Send explicit RequestCatalog to the peer
        if let Err(e) = node.request_catalog_from_peer(nid).await {
            warn!(peer = %peer_node_id, "failed to request catalog from peer: {e}");
            // Don't abort — the Ping handler may have already triggered a catalog send
        }

        // Wait for incoming tracks from the peer (they arrive asynchronously via
        // the CatalogSync handler). Poll the remote_track table until we reach the
        // expected count or timeout.
        if expected_tracks > 0 {
            let db = node.db();
            let domain = normalize_instance_domain(&peer_node_id);
            let poll_interval = std::time::Duration::from_secs(2);
            // Scale timeout dynamically: 2s per track, min 5m, max 2h
            let timeout_secs = (expected_tracks * 2).clamp(300, 7200);
            let timeout = std::time::Duration::from_secs(timeout_secs);
            let deadline = std::time::Instant::now() + timeout;
            let mut last_count = 0u64;
            let mut stall_count = 0u32;

            // Scale stall threshold: allow more time between pages for large catalogs.
            // Each page of 500 tracks can take 25-60s to process, so we allow
            // (pages * 5 + 30) polls × 2s each.
            let max_stall = ((expected_tracks / 500) * 5 + 30) as u32;

            loop {
                tokio::time::sleep(poll_interval).await;

                let current_count = remote_track::Entity::find()
                    .filter(remote_track::Column::InstanceDomain.eq(&domain))
                    .count(db)
                    .await
                    .unwrap_or(0);

                // Update progress
                {
                    let mut status = tracker.lock().await;
                    *status = LibrarySyncTaskStatus::Running {
                        peer_id: peer_node_id.clone(),
                        progress: SyncProgress {
                            processed: current_count,
                            total: Some(expected_tracks),
                            phase: format!(
                                "Receiving catalog... ({}/{})",
                                current_count, expected_tracks
                            ),
                        },
                    };
                }

                if current_count >= expected_tracks {
                    info!(
                        peer = %peer_node_id,
                        tracks = current_count,
                        "all expected tracks received from peer"
                    );
                    break;
                }

                // Detect stall: if count hasn't changed for 10 consecutive polls (20s)
                if current_count == last_count {
                    stall_count += 1;
                    if stall_count >= max_stall {
                        info!(
                            peer = %peer_node_id,
                            received = current_count,
                            expected = expected_tracks,
                            "catalog reception stalled — continuing with received tracks"
                        );
                        break;
                    }
                } else {
                    stall_count = 0;
                }
                last_count = current_count;

                if std::time::Instant::now() >= deadline {
                    warn!(
                        peer = %peer_node_id,
                        received = current_count,
                        expected = expected_tracks,
                        timeout_secs,
                        "catalog reception timed out"
                    );
                    break;
                }
            }
        }

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

        // Phase 5: Incremental sync to send any missing data
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
        // Pass sync start time so only tracks added DURING the sync are sent
        // (Phase 2 already sent the full catalog — this avoids duplicating 24K announcements)
        let sync_start_chrono =
            chrono::Utc::now() - chrono::Duration::seconds(start.elapsed().as_secs() as i64);
        node.incremental_sync_to_peer(nid, Some(sync_start_chrono))
            .await;

        // Count final results
        let db = node.db();
        let domain = normalize_instance_domain(&peer_node_id);
        let tracks_synced = remote_track::Entity::find()
            .filter(remote_track::Column::InstanceDomain.eq(&domain))
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
                    tracks_already_known: 0,
                    errors: 0,
                    duration_secs: (elapsed * 100.0).round() / 100.0,
                },
            };
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── normalize_instance_domain ───────────────────────────────────

    #[test]
    fn test_normalize_instance_domain_bare_id() {
        assert_eq!(normalize_instance_domain("abc123"), "p2p://abc123");
    }

    #[test]
    fn test_normalize_instance_domain_already_prefixed() {
        assert_eq!(normalize_instance_domain("p2p://abc123"), "p2p://abc123");
    }

    #[test]
    fn test_normalize_instance_domain_empty() {
        assert_eq!(normalize_instance_domain(""), "p2p://");
    }

    #[test]
    fn test_normalize_instance_domain_long_id() {
        let long_id = "a".repeat(64);
        assert_eq!(
            normalize_instance_domain(&long_id),
            format!("p2p://{long_id}")
        );
    }

    // ─── SyncState serde ─────────────────────────────────────────────

    #[test]
    fn test_sync_state_serialize_synced() {
        let val = serde_json::to_value(SyncState::Synced).unwrap();
        assert_eq!(val, "synced");
    }

    #[test]
    fn test_sync_state_serialize_partial() {
        let val = serde_json::to_value(SyncState::Partial).unwrap();
        assert_eq!(val, "partial");
    }

    #[test]
    fn test_sync_state_serialize_not_synced() {
        let val = serde_json::to_value(SyncState::NotSynced).unwrap();
        assert_eq!(val, "not_synced");
    }

    #[test]
    fn test_sync_state_serialize_offline() {
        let val = serde_json::to_value(SyncState::Offline).unwrap();
        assert_eq!(val, "offline");
    }

    #[test]
    fn test_sync_state_serialize_empty() {
        let val = serde_json::to_value(SyncState::Empty).unwrap();
        assert_eq!(val, "empty");
    }

    // ─── SyncState equality ──────────────────────────────────────────

    #[test]
    fn test_sync_state_equality() {
        assert_eq!(SyncState::Synced, SyncState::Synced);
        assert_ne!(SyncState::Synced, SyncState::Partial);
        assert_ne!(SyncState::Offline, SyncState::Empty);
    }

    // ─── LibrarySyncTaskStatus serde ─────────────────────────────────

    #[test]
    fn test_task_status_idle_serialize() {
        let val = serde_json::to_value(LibrarySyncTaskStatus::Idle).unwrap();
        assert_eq!(val["status"], "idle");
    }

    #[test]
    fn test_task_status_running_serialize() {
        let status = LibrarySyncTaskStatus::Running {
            peer_id: "abc123".to_string(),
            progress: SyncProgress {
                processed: 10,
                total: Some(100),
                phase: "Syncing...".to_string(),
            },
        };
        let val = serde_json::to_value(&status).unwrap();
        assert_eq!(val["status"], "running");
        assert_eq!(val["peer_id"], "abc123");
        assert_eq!(val["progress"]["processed"], 10);
        assert_eq!(val["progress"]["total"], 100);
        assert_eq!(val["progress"]["phase"], "Syncing...");
    }

    #[test]
    fn test_task_status_completed_serialize() {
        let status = LibrarySyncTaskStatus::Completed {
            result: SyncResult {
                peer_id: "abc123".to_string(),
                tracks_synced: 50,
                tracks_already_known: 30,
                errors: 2,
                duration_secs: 12.34,
            },
        };
        let val = serde_json::to_value(&status).unwrap();
        assert_eq!(val["status"], "completed");
        assert_eq!(val["result"]["tracks_synced"], 50);
        assert_eq!(val["result"]["tracks_already_known"], 30);
        assert_eq!(val["result"]["errors"], 2);
    }

    #[test]
    fn test_task_status_error_serialize() {
        let status = LibrarySyncTaskStatus::Error {
            message: "Connection refused".to_string(),
        };
        let val = serde_json::to_value(&status).unwrap();
        assert_eq!(val["status"], "error");
        assert_eq!(val["message"], "Connection refused");
    }

    // ─── SyncProgress serde ──────────────────────────────────────────

    #[test]
    fn test_sync_progress_serialize() {
        let progress = SyncProgress {
            processed: 42,
            total: Some(100),
            phase: "Downloading".to_string(),
        };
        let val = serde_json::to_value(&progress).unwrap();
        assert_eq!(val["processed"], 42);
        assert_eq!(val["total"], 100);
        assert_eq!(val["phase"], "Downloading");
    }

    #[test]
    fn test_sync_progress_serialize_no_total() {
        let progress = SyncProgress {
            processed: 0,
            total: None,
            phase: "Starting".to_string(),
        };
        let val = serde_json::to_value(&progress).unwrap();
        assert_eq!(val["processed"], 0);
        assert!(val["total"].is_null());
    }

    // ─── SyncResult serde ────────────────────────────────────────────

    #[test]
    fn test_sync_result_serialize() {
        let result = SyncResult {
            peer_id: "xyz789".to_string(),
            tracks_synced: 100,
            tracks_already_known: 50,
            errors: 0,
            duration_secs: 5.67,
        };
        let val = serde_json::to_value(&result).unwrap();
        assert_eq!(val["peer_id"], "xyz789");
        assert_eq!(val["tracks_synced"], 100);
        assert_eq!(val["duration_secs"], 5.67);
    }

    // ─── PeerSyncStatus serde ────────────────────────────────────────

    #[test]
    fn test_peer_sync_status_serialize() {
        let status = PeerSyncStatus {
            node_id: "abc".to_string(),
            name: Some("TestPeer".to_string()),
            version: Some("0.1.0".to_string()),
            is_online: true,
            peer_announced_tracks: 100,
            local_remote_tracks: 80,
            available_tracks: 75,
            our_track_count: 50,
            sync_ratio: 0.80,
            sync_state: SyncState::Partial,
            last_seen: "2024-01-01T00:00:00Z".to_string(),
        };
        let val = serde_json::to_value(&status).unwrap();
        assert_eq!(val["node_id"], "abc");
        assert_eq!(val["name"], "TestPeer");
        assert_eq!(val["sync_state"], "partial");
        assert_eq!(val["peer_announced_tracks"], 100);
    }

    #[test]
    fn test_peer_sync_status_serialize_offline() {
        let status = PeerSyncStatus {
            node_id: "xyz".to_string(),
            name: None,
            version: None,
            is_online: false,
            peer_announced_tracks: 0,
            local_remote_tracks: 0,
            available_tracks: 0,
            our_track_count: 10,
            sync_ratio: 0.0,
            sync_state: SyncState::Offline,
            last_seen: "2024-01-01T00:00:00Z".to_string(),
        };
        let val = serde_json::to_value(&status).unwrap();
        assert!(val["name"].is_null());
        assert_eq!(val["sync_state"], "offline");
    }

    // ─── LibrarySyncOverview serde ───────────────────────────────────

    #[test]
    fn test_library_sync_overview_serialize_empty() {
        let overview = LibrarySyncOverview {
            local_track_count: 0,
            total_peers: 0,
            synced_peers: 0,
            partial_peers: 0,
            not_synced_peers: 0,
            peers: vec![],
        };
        let val = serde_json::to_value(&overview).unwrap();
        assert_eq!(val["local_track_count"], 0);
        assert_eq!(val["total_peers"], 0);
        assert!(val["peers"].as_array().unwrap().is_empty());
    }

    // ─── new_sync_tracker ────────────────────────────────────────────

    #[tokio::test]
    async fn test_new_sync_tracker_starts_idle() {
        let tracker = new_sync_tracker();
        let status = tracker.lock().await;
        match &*status {
            LibrarySyncTaskStatus::Idle => {} // expected
            other => panic!("expected Idle, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_sync_tracker_can_be_mutated() {
        let tracker = new_sync_tracker();

        {
            let mut status = tracker.lock().await;
            *status = LibrarySyncTaskStatus::Running {
                peer_id: "test".to_string(),
                progress: SyncProgress {
                    processed: 0,
                    total: None,
                    phase: "Testing".to_string(),
                },
            };
        }

        {
            let status = tracker.lock().await;
            match &*status {
                LibrarySyncTaskStatus::Running { peer_id, .. } => {
                    assert_eq!(peer_id, "test");
                }
                other => panic!("expected Running, got {:?}", other),
            }
        }
    }

    // ─── LibrarySyncTaskStatus clone ─────────────────────────────────

    #[test]
    fn test_task_status_clone() {
        let status = LibrarySyncTaskStatus::Completed {
            result: SyncResult {
                peer_id: "abc".to_string(),
                tracks_synced: 10,
                tracks_already_known: 5,
                errors: 0,
                duration_secs: 1.0,
            },
        };
        let cloned = status.clone();
        let val = serde_json::to_value(&cloned).unwrap();
        assert_eq!(val["status"], "completed");
        assert_eq!(val["result"]["tracks_synced"], 10);
    }
}
