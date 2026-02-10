//! soundtime-p2p — P2P networking layer for SoundTime using iroh.
//!
//! Provides autonomous peer discovery via iroh (QUIC-based P2P),
//! content-addressed track sharing via iroh-blobs,
//! admin-configurable peer blocking,
//! bloom-filter search routing, and
//! distributed search across the network.

pub mod blocked;
pub mod discovery;
pub mod error;
pub mod library_sync;
pub mod musicbrainz;
pub mod node;
pub mod search_index;
pub mod track_health;

pub use discovery::{PeerInfo, PeerRegistry};
pub use error::P2pError;
pub use musicbrainz::MusicBrainzClient;
pub use node::{P2pConfig, P2pMessage, P2pNode, SearchResultItem, TrackAnnouncement};
pub use search_index::{BloomFilterData, SearchIndex};
pub use track_health::{
    auto_repair_on_failure, persist_track_status, run_health_sweep, spawn_health_monitor,
    BatchCheckResult, HealthMonitorConfig, HealthStatus, PeerTrackInfo, RecoveryResult,
    TrackCheckItem, TrackFetcher, TrackHealthManager,
};
pub use library_sync::{
    get_library_sync_overview, new_sync_tracker, spawn_library_resync, LibrarySyncOverview,
    LibrarySyncTaskStatus, PeerSyncStatus, SyncProgress, SyncResult, SyncState, SyncTaskHandle,
};

// Re-export iroh types needed by consumers
pub use iroh::{EndpointAddr, EndpointId};
/// Backward-compatible type aliases
pub type NodeAddr = EndpointAddr;
pub type NodeId = EndpointId;
pub use iroh_blobs::Hash as BlobHash;

/// Return the build version string.
///
/// Uses `CARGO_PKG_VERSION` (from Cargo.toml) as the base.
/// If `SOUNDTIME_BUILD_NUMBER` is set at compile time (via CI),
/// the patch component is replaced: e.g. `0.1.0` + build 42 → `0.1.42`.
pub fn build_version() -> &'static str {
    // If the CI injects a build number, the Dockerfile passes it
    // as `SOUNDTIME_BUILD_NUMBER` env var at compile time.
    // We embed it via option_env!() which is resolved at compile time.
    static VERSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    VERSION.get_or_init(|| {
        let base = env!("CARGO_PKG_VERSION"); // e.g. "0.1.0"
        match option_env!("SOUNDTIME_BUILD_NUMBER") {
            Some(build) if !build.is_empty() => {
                // Replace patch: "0.1.0" → "0.1.<build>"
                let parts: Vec<&str> = base.splitn(3, '.').collect();
                if parts.len() >= 2 {
                    format!("{}.{}.{}", parts[0], parts[1], build)
                } else {
                    format!("{base}+{build}")
                }
            }
            _ => base.to_string(),
        }
    })
}
