//! P2P error types.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum P2pError {
    #[error("iroh endpoint error: {0}")]
    Endpoint(String),

    #[error("blob store error: {0}")]
    BlobStore(String),

    #[error("peer is blocked: {0}")]
    PeerBlocked(String),

    #[error("track not found: {0}")]
    TrackNotFound(String),

    #[error("database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("connection error: {0}")]
    Connection(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
