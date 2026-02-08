//! soundtime-p2p — P2P networking layer for SoundTime using iroh.
//!
//! Provides autonomous peer discovery via iroh (QUIC-based P2P),
//! content-addressed track sharing via iroh-blobs, and
//! admin-configurable peer blocking.

pub mod blocked;
pub mod discovery;
pub mod error;
pub mod node;

pub use discovery::{PeerInfo, PeerRegistry};
pub use error::P2pError;
pub use node::{P2pConfig, P2pMessage, P2pNode, TrackAnnouncement};

// Re-export iroh types needed by consumers
pub use iroh::{NodeAddr, NodeId, RelayUrl};
pub use iroh_blobs::Hash as BlobHash;
