//! Plugin system error types.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum PluginError {
    #[error("manifest error: {0}")]
    Manifest(String),

    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("sandbox error: {0}")]
    Sandbox(String),

    #[error("plugin not found: {0}")]
    NotFound(String),

    #[error("plugin already exists: {0}")]
    AlreadyExists(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("host function error: {0}")]
    HostFunction(String),

    #[error("installation error: {0}")]
    Installation(String),

    #[error("git error: {0}")]
    Git(#[from] git2::Error),

    #[error("WASM validation error: {0}")]
    WasmValidation(String),

    #[error("execution timeout: plugin {0} exceeded fuel limit")]
    FuelExhausted(String),

    #[error("memory limit exceeded: plugin {0}")]
    MemoryExceeded(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("database error: {0}")]
    Database(#[from] sea_orm::DbErr),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("semver error: {0}")]
    Semver(#[from] semver::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    // ── Display messages ──────────────────────────────────────────────

    #[test]
    fn test_display_manifest() {
        let err = PluginError::Manifest("missing name field".into());
        assert_eq!(err.to_string(), "manifest error: missing name field");
    }

    #[test]
    fn test_display_invalid_manifest() {
        let err = PluginError::InvalidManifest("bad version".into());
        assert_eq!(err.to_string(), "invalid manifest: bad version");
    }

    #[test]
    fn test_display_sandbox() {
        let err = PluginError::Sandbox("wasm trap".into());
        assert_eq!(err.to_string(), "sandbox error: wasm trap");
    }

    #[test]
    fn test_display_not_found() {
        let err = PluginError::NotFound("my-plugin".into());
        assert_eq!(err.to_string(), "plugin not found: my-plugin");
    }

    #[test]
    fn test_display_already_exists() {
        let err = PluginError::AlreadyExists("my-plugin".into());
        assert_eq!(err.to_string(), "plugin already exists: my-plugin");
    }

    #[test]
    fn test_display_permission_denied() {
        let err = PluginError::PermissionDenied("http_get requires http_hosts".into());
        assert_eq!(
            err.to_string(),
            "permission denied: http_get requires http_hosts"
        );
    }

    #[test]
    fn test_display_host_function() {
        let err = PluginError::HostFunction("get_track failed".into());
        assert_eq!(err.to_string(), "host function error: get_track failed");
    }

    #[test]
    fn test_display_installation() {
        let err = PluginError::Installation("clone failed".into());
        assert_eq!(err.to_string(), "installation error: clone failed");
    }

    #[test]
    fn test_display_wasm_validation() {
        let err = PluginError::WasmValidation("unauthorized import".into());
        assert_eq!(
            err.to_string(),
            "WASM validation error: unauthorized import"
        );
    }

    #[test]
    fn test_display_fuel_exhausted() {
        let err = PluginError::FuelExhausted("my-plugin".into());
        assert_eq!(
            err.to_string(),
            "execution timeout: plugin my-plugin exceeded fuel limit"
        );
    }

    #[test]
    fn test_display_memory_exceeded() {
        let err = PluginError::MemoryExceeded("my-plugin".into());
        assert_eq!(err.to_string(), "memory limit exceeded: plugin my-plugin");
    }

    #[test]
    fn test_display_http() {
        let err = PluginError::Http("timeout".into());
        assert_eq!(err.to_string(), "HTTP error: timeout");
    }

    // ── From conversions ──────────────────────────────────────────────

    #[test]
    fn test_from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file missing");
        let err: PluginError = io_err.into();
        assert!(matches!(err, PluginError::Io(_)));
        assert!(err.to_string().contains("file missing"));
    }

    #[test]
    fn test_from_serde_json_error() {
        let json_err = serde_json::from_str::<String>("bad json{{{").unwrap_err();
        let err: PluginError = json_err.into();
        assert!(matches!(err, PluginError::Serialization(_)));
    }

    #[test]
    fn test_from_toml_error() {
        let toml_err = toml::from_str::<toml::Value>("= bad").unwrap_err();
        let err: PluginError = toml_err.into();
        assert!(matches!(err, PluginError::TomlParse(_)));
    }

    #[test]
    fn test_from_db_error() {
        let db_err = sea_orm::DbErr::Custom("test db error".into());
        let err: PluginError = db_err.into();
        assert!(matches!(err, PluginError::Database(_)));
    }

    #[test]
    fn test_from_semver_error() {
        let sv_err = "not.a.version".parse::<semver::Version>().unwrap_err();
        let err: PluginError = sv_err.into();
        assert!(matches!(err, PluginError::Semver(_)));
    }

    // ── Debug impl ────────────────────────────────────────────────────

    #[test]
    fn test_debug_formatting() {
        let err = PluginError::NotFound("test".into());
        let debug = format!("{:?}", err);
        assert!(debug.contains("NotFound"));
        assert!(debug.contains("test"));
    }

    // ── Error trait source chain ──────────────────────────────────────

    #[test]
    fn test_error_source_io() {
        use std::error::Error;
        let io_err = io::Error::new(io::ErrorKind::BrokenPipe, "pipe broken");
        let err: PluginError = io_err.into();
        assert!(err.source().is_some());
    }

    #[test]
    fn test_error_source_string_variants() {
        use std::error::Error;
        let err = PluginError::Sandbox("timeout".into());
        assert!(err.source().is_none());
    }
}
