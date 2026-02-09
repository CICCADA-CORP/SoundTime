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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    // ── Display messages ──────────────────────────────────────────────

    #[test]
    fn test_display_endpoint() {
        let err = P2pError::Endpoint("connection refused".into());
        assert_eq!(err.to_string(), "iroh endpoint error: connection refused");
    }

    #[test]
    fn test_display_blob_store() {
        let err = P2pError::BlobStore("corrupt data".into());
        assert_eq!(err.to_string(), "blob store error: corrupt data");
    }

    #[test]
    fn test_display_peer_blocked() {
        let err = P2pError::PeerBlocked("abc123".into());
        assert_eq!(err.to_string(), "peer is blocked: abc123");
    }

    #[test]
    fn test_display_track_not_found() {
        let err = P2pError::TrackNotFound("deadbeef".into());
        assert_eq!(err.to_string(), "track not found: deadbeef");
    }

    #[test]
    fn test_display_connection() {
        let err = P2pError::Connection("timeout".into());
        assert_eq!(err.to_string(), "connection error: timeout");
    }

    // ── From conversions ──────────────────────────────────────────────

    #[test]
    fn test_from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file missing");
        let p2p_err: P2pError = io_err.into();
        assert!(matches!(p2p_err, P2pError::Io(_)));
        assert!(p2p_err.to_string().contains("file missing"));
    }

    #[test]
    fn test_from_serde_json_error() {
        let json_err = serde_json::from_str::<String>("not json{{{").unwrap_err();
        let p2p_err: P2pError = json_err.into();
        assert!(matches!(p2p_err, P2pError::Serialization(_)));
        assert!(p2p_err.to_string().starts_with("serialization error:"));
    }

    #[test]
    fn test_from_db_error() {
        let db_err = sea_orm::DbErr::Custom("test db error".into());
        let p2p_err: P2pError = db_err.into();
        assert!(matches!(p2p_err, P2pError::Database(_)));
        assert!(p2p_err.to_string().contains("test db error"));
    }

    // ── Debug impl ────────────────────────────────────────────────────

    #[test]
    fn test_debug_formatting() {
        let err = P2pError::Endpoint("test".into());
        let debug = format!("{:?}", err);
        assert!(debug.contains("Endpoint"));
        assert!(debug.contains("test"));
    }

    // ── Error trait source chain ──────────────────────────────────────

    #[test]
    fn test_error_source_io() {
        use std::error::Error;
        let io_err = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broken");
        let p2p_err: P2pError = io_err.into();
        assert!(p2p_err.source().is_some());
    }

    #[test]
    fn test_error_source_string_variants() {
        use std::error::Error;
        // String-based variants have no source
        let err = P2pError::Connection("timeout".into());
        assert!(err.source().is_none());
    }
}
