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

// Re-export iroh types needed by consumers
pub use iroh::{EndpointAddr, EndpointId};
/// Backward-compatible type aliases
pub type NodeAddr = EndpointAddr;
pub type NodeId = EndpointId;
pub use iroh_blobs::Hash as BlobHash;
