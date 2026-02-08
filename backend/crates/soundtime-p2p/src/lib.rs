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

pub use discovery::{PeerInfo, PeerRegistry};
pub use error::P2pError;
pub use musicbrainz::MusicBrainzClient;
pub use node::{P2pConfig, P2pMessage, P2pNode, SearchResultItem, TrackAnnouncement};
pub use search_index::{BloomFilterData, SearchIndex};

// Re-export iroh types needed by consumers
pub use iroh::{NodeAddr, NodeId, RelayUrl};
pub use iroh_blobs::Hash as BlobHash;
