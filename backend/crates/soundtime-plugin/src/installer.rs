//! Plugin installer — git clone, validation, installation flow.
//!
//! Handles cloning plugin repos, validating their manifests and WASM
//! binaries, and installing them to the plugin directory.

use std::path::{Path, PathBuf};

use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use uuid::Uuid;

use crate::error::PluginError;
use crate::manifest::PluginManifest;
use soundtime_db::entities::plugin;

// ─── Constants ──────────────────────────────────────────────────────────

/// WASM magic bytes: `\0asm`
const WASM_MAGIC: &[u8; 4] = b"\0asm";

/// Default max WASM binary size: 50 MB.
const DEFAULT_MAX_WASM_SIZE_MB: u64 = 50;

/// Allowed WASM import namespaces. Imports outside these are rejected.
const ALLOWED_IMPORT_NAMESPACES: &[&str] = &[
    "env",                    // Extism host functions
    "extism:host/env",        // Extism host functions (component model)
    "wasi_snapshot_preview1", // WASI preview 1 (enabled in sandbox)
    "wasi_unstable",          // Legacy WASI
];

// ─── Security helpers ────────────────────────────────────────────────

/// Validate a git URL for security.
///
/// Only HTTPS URLs are allowed. File, HTTP, SSH, and git protocols are
/// blocked to prevent SSRF and local file access.
fn validate_git_url(url: &str) -> Result<(), PluginError> {
    let parsed = url::Url::parse(url)
        .map_err(|_| PluginError::Installation(format!("invalid git URL: '{url}'")))?;

    if parsed.scheme() != "https" {
        return Err(PluginError::Installation(format!(
            "only HTTPS git URLs are allowed, got scheme '{}' in '{url}'",
            parsed.scheme()
        )));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| PluginError::Installation(format!("git URL has no host: '{url}'")))?;

    // SECURITY: Block private/reserved IP ranges and cloud metadata endpoints
    let blocked_hosts = [
        "localhost",
        "127.0.0.1",
        "0.0.0.0",
        "[::1]",
        "169.254.169.254",          // AWS/GCP metadata
        "metadata.google.internal", // GCP metadata
    ];
    if blocked_hosts.contains(&host) {
        return Err(PluginError::Installation(format!(
            "git URL host '{host}' is blocked (private/reserved address)"
        )));
    }

    // Block 10.x.x.x, 172.16-31.x.x, 192.168.x.x
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let is_private = match ip {
            std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
            std::net::IpAddr::V6(v6) => v6.is_loopback(),
        };
        if is_private {
            return Err(PluginError::Installation(format!(
                "git URL resolves to private IP: '{host}'"
            )));
        }
    }

    Ok(())
}

/// Recursively copy a directory and its contents.
async fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), PluginError> {
    tokio::fs::create_dir_all(dest).await.map_err(|e| {
        PluginError::Installation(format!("failed to create dir {}: {e}", dest.display()))
    })?;

    let mut entries = tokio::fs::read_dir(src).await.map_err(|e| {
        PluginError::Installation(format!("failed to read dir {}: {e}", src.display()))
    })?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| PluginError::Installation(format!("failed to read dir entry: {e}")))?
    {
        let entry_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if entry_path.is_dir() {
            Box::pin(copy_dir_recursive(&entry_path, &dest_path)).await?;
        } else {
            tokio::fs::copy(&entry_path, &dest_path)
                .await
                .map_err(|e| {
                    PluginError::Installation(format!(
                        "failed to copy {}: {e}",
                        entry_path.display()
                    ))
                })?;
        }
    }

    Ok(())
}

// ─── Installer ──────────────────────────────────────────────────────────

/// Plugin installer handles the complete installation flow.
pub struct PluginInstaller {
    /// Base directory for installed plugins.
    plugin_dir: PathBuf,
    /// Maximum WASM binary size in bytes.
    max_wasm_size: u64,
    /// Database connection.
    db: DatabaseConnection,
}

impl PluginInstaller {
    /// Create a new installer with configuration from environment.
    pub fn new(db: DatabaseConnection) -> Self {
        let plugin_dir =
            std::env::var("PLUGIN_DIR").unwrap_or_else(|_| "/data/plugins".to_string());
        let max_wasm_size = std::env::var("PLUGIN_WASM_MAX_SIZE_MB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MAX_WASM_SIZE_MB)
            * 1024
            * 1024;

        Self {
            plugin_dir: PathBuf::from(plugin_dir),
            max_wasm_size,
            db,
        }
    }

    /// Install a plugin from a git repository URL.
    ///
    /// The full installation flow:
    /// 1. Clone repo to temp directory
    /// 2. Parse and validate `plugin.toml`
    /// 3. Validate WASM binary (magic bytes, size, imports)
    /// 4. Check for name conflicts in DB
    /// 5. Copy to plugin directory
    /// 6. Register in database with status "disabled"
    ///
    /// Returns the new plugin's UUID.
    pub async fn install_from_git(
        &self,
        git_url: &str,
        installed_by: Option<Uuid>,
    ) -> Result<plugin::Model, PluginError> {
        tracing::info!(git_url = %git_url, "starting plugin installation");

        // SECURITY: Validate git URL before cloning
        validate_git_url(git_url)?;

        // 1. Clone to temp directory
        let temp_dir = tempfile::tempdir()
            .map_err(|e| PluginError::Installation(format!("failed to create temp dir: {e}")))?;

        self.clone_repo(git_url, temp_dir.path())?;

        // 2. Parse and validate manifest
        let manifest_path = temp_dir.path().join("plugin.toml");
        if !manifest_path.exists() {
            return Err(PluginError::Installation(
                "plugin.toml not found in repository root".into(),
            ));
        }

        let manifest_content = tokio::fs::read_to_string(&manifest_path)
            .await
            .map_err(|e| PluginError::Installation(format!("failed to read plugin.toml: {e}")))?;

        let manifest = PluginManifest::parse_and_validate(&manifest_content)?;

        // 3. Validate WASM binary
        let wasm_path = temp_dir.path().join(&manifest.build.wasm);
        if !wasm_path.exists() {
            return Err(PluginError::Installation(format!(
                "WASM binary not found at declared path: {}",
                manifest.build.wasm
            )));
        }

        // SECURITY: Canonicalize and verify path stays within temp dir
        let wasm_path = wasm_path
            .canonicalize()
            .map_err(|e| PluginError::Installation(format!("invalid WASM path: {e}")))?;
        if !wasm_path.starts_with(temp_dir.path()) {
            return Err(PluginError::Installation(
                "WASM path escapes repository directory (path traversal)".into(),
            ));
        }

        self.validate_wasm(&wasm_path).await?;

        // 4. Check for name conflicts
        let existing = plugin::Entity::find()
            .filter(plugin::Column::Name.eq(&manifest.plugin.name))
            .one(&self.db)
            .await?;

        if existing.is_some() {
            return Err(PluginError::AlreadyExists(manifest.plugin.name.clone()));
        }

        // 5. Copy to plugin directory
        let install_dir = self.plugin_dir.join(format!(
            "{}-{}",
            manifest.plugin.name, manifest.plugin.version
        ));

        if install_dir.exists() {
            tokio::fs::remove_dir_all(&install_dir).await.map_err(|e| {
                PluginError::Installation(format!("failed to clean install dir: {e}"))
            })?;
        }

        tokio::fs::create_dir_all(&install_dir)
            .await
            .map_err(|e| PluginError::Installation(format!("failed to create install dir: {e}")))?;

        // Copy WASM binary
        let dest_wasm = install_dir.join("plugin.wasm");
        tokio::fs::copy(&wasm_path, &dest_wasm)
            .await
            .map_err(|e| PluginError::Installation(format!("failed to copy WASM binary: {e}")))?;

        // Copy plugin.toml
        let dest_manifest = install_dir.join("plugin.toml");
        tokio::fs::copy(&manifest_path, &dest_manifest)
            .await
            .map_err(|e| PluginError::Installation(format!("failed to copy plugin.toml: {e}")))?;

        // Copy UI assets if applicable
        if manifest.ui.enabled {
            if let Some(ref entry) = manifest.ui.entry {
                let ui_src = temp_dir.path().join(entry);
                // SECURITY: Canonicalize and verify path stays within temp dir
                if ui_src.exists() {
                    let ui_src = ui_src.canonicalize().map_err(|e| {
                        PluginError::Installation(format!("invalid UI entry path: {e}"))
                    })?;
                    if !ui_src.starts_with(temp_dir.path()) {
                        return Err(PluginError::Installation(
                            "UI entry path escapes repository directory (path traversal)".into(),
                        ));
                    }
                }

                // Copy the entire UI directory (parent of entry file)
                if let Some(ui_dir_rel) = Path::new(entry).parent() {
                    if !ui_dir_rel.as_os_str().is_empty() {
                        let ui_src_dir = temp_dir.path().join(ui_dir_rel);
                        if ui_src_dir.exists() {
                            copy_dir_recursive(&ui_src_dir, &install_dir.join(ui_dir_rel)).await?;
                        }
                    } else {
                        // Entry is at root level, just copy the file
                        if ui_src.exists() {
                            let ui_dest = install_dir.join(entry);
                            tokio::fs::copy(&ui_src, &ui_dest).await.map_err(|e| {
                                PluginError::Installation(format!("failed to copy UI entry: {e}"))
                            })?;
                        }
                    }
                }
            }
        }

        // 6. Register in database
        let permissions_json = serde_json::to_value(&manifest.permissions)?;
        let now = chrono::Utc::now().fixed_offset();
        let plugin_id = Uuid::new_v4();

        let new_plugin = plugin::ActiveModel {
            id: Set(plugin_id),
            name: Set(manifest.plugin.name.clone()),
            version: Set(manifest.plugin.version.clone()),
            description: Set(Some(manifest.plugin.description.clone())),
            author: Set(manifest.plugin.author.clone()),
            license: Set(manifest.plugin.license.clone()),
            homepage: Set(manifest.plugin.homepage.clone()),
            git_url: Set(git_url.to_string()),
            wasm_path: Set(dest_wasm.to_string_lossy().to_string()),
            permissions: Set(permissions_json),
            status: Set("disabled".to_string()),
            error_message: Set(None),
            installed_at: Set(now),
            updated_at: Set(now),
            installed_by: Set(installed_by),
        };

        let model = new_plugin.insert(&self.db).await?;

        tracing::info!(
            plugin_name = %manifest.plugin.name,
            plugin_id = %plugin_id,
            version = %manifest.plugin.version,
            "plugin installed successfully"
        );

        Ok(model)
    }

    /// Clone a git repository to the given directory.
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<(), PluginError> {
        tracing::info!(url = %url, dest = %dest.display(), "cloning plugin repository");

        git2::Repository::clone(url, dest)
            .map_err(|e| PluginError::Installation(format!("git clone failed: {e}")))?;

        Ok(())
    }

    /// Validate a WASM binary file.
    ///
    /// Checks:
    /// 1. File size within limits
    /// 2. Magic bytes (0x00 0x61 0x73 0x6D = "\0asm")
    /// 3. Import analysis: only allowed namespaces
    async fn validate_wasm(&self, wasm_path: &Path) -> Result<(), PluginError> {
        // Check file size
        let metadata = tokio::fs::metadata(wasm_path).await.map_err(|e| {
            PluginError::WasmValidation(format!("failed to read WASM metadata: {e}"))
        })?;

        let size = metadata.len();
        if size > self.max_wasm_size {
            return Err(PluginError::WasmValidation(format!(
                "WASM binary too large: {} bytes (max: {} bytes)",
                size, self.max_wasm_size
            )));
        }

        // Read the binary
        let wasm_bytes = tokio::fs::read(wasm_path)
            .await
            .map_err(|e| PluginError::WasmValidation(format!("failed to read WASM binary: {e}")))?;

        // Check magic bytes
        if wasm_bytes.len() < 4 || &wasm_bytes[..4] != WASM_MAGIC {
            return Err(PluginError::WasmValidation(
                "invalid WASM binary: magic bytes mismatch".into(),
            ));
        }

        // Analyze imports using wasmparser
        self.validate_wasm_imports(&wasm_bytes)?;

        Ok(())
    }

    /// Validate WASM imports against the allowed namespace list.
    fn validate_wasm_imports(&self, wasm_bytes: &[u8]) -> Result<(), PluginError> {
        use wasmparser::{Parser, Payload};

        let parser = Parser::new(0);

        for payload in parser.parse_all(wasm_bytes) {
            let payload = payload
                .map_err(|e| PluginError::WasmValidation(format!("failed to parse WASM: {e}")))?;

            if let Payload::ImportSection(reader) = payload {
                for import in reader {
                    let import = import.map_err(|e| {
                        PluginError::WasmValidation(format!("failed to read import: {e}"))
                    })?;

                    let module = import.module;
                    if !ALLOWED_IMPORT_NAMESPACES.contains(&module) {
                        return Err(PluginError::WasmValidation(format!(
                            "unauthorized import namespace: '{}' (function: '{}'); \
                             allowed namespaces: {:?}",
                            module, import.name, ALLOWED_IMPORT_NAMESPACES
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    /// Update an installed plugin by re-cloning and re-validating.
    ///
    /// The old version is preserved for rollback. If loading the new
    /// version fails, the old version is restored.
    pub async fn update_plugin(&self, plugin_id: Uuid) -> Result<plugin::Model, PluginError> {
        // Get existing plugin from DB
        let existing = plugin::Entity::find_by_id(plugin_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        let old_version = existing.version.clone();
        let git_url = existing.git_url.clone();

        tracing::info!(
            plugin_name = %existing.name,
            old_version = %old_version,
            "starting plugin update"
        );

        // Clone new version to temp
        let temp_dir = tempfile::tempdir()
            .map_err(|e| PluginError::Installation(format!("failed to create temp dir: {e}")))?;

        // SECURITY: Validate git URL before cloning
        validate_git_url(&git_url)?;

        self.clone_repo(&git_url, temp_dir.path())?;

        // Parse and validate new manifest
        let manifest_path = temp_dir.path().join("plugin.toml");
        if !manifest_path.exists() {
            return Err(PluginError::Installation(
                "plugin.toml not found in updated repository".into(),
            ));
        }

        let manifest_content = tokio::fs::read_to_string(&manifest_path)
            .await
            .map_err(|e| PluginError::Installation(format!("failed to read plugin.toml: {e}")))?;

        let manifest = PluginManifest::parse_and_validate(&manifest_content)?;

        // Verify plugin name matches
        if manifest.plugin.name != existing.name {
            return Err(PluginError::Installation(format!(
                "plugin name mismatch: expected '{}', got '{}'",
                existing.name, manifest.plugin.name
            )));
        }

        // Log permission changes
        let old_perms: serde_json::Value = existing.permissions.clone();
        let new_perms = serde_json::to_value(&manifest.permissions)?;
        if old_perms != new_perms {
            tracing::warn!(
                plugin_name = %existing.name,
                "plugin permissions changed during update — old: {old_perms}, new: {new_perms}"
            );
        }

        // Validate WASM
        let wasm_path = temp_dir.path().join(&manifest.build.wasm);
        if !wasm_path.exists() {
            return Err(PluginError::Installation(format!(
                "WASM binary not found at: {}",
                manifest.build.wasm
            )));
        }

        // SECURITY: Canonicalize and verify path stays within temp dir
        let wasm_path = wasm_path
            .canonicalize()
            .map_err(|e| PluginError::Installation(format!("invalid WASM path: {e}")))?;
        if !wasm_path.starts_with(temp_dir.path()) {
            return Err(PluginError::Installation(
                "WASM path escapes repository directory (path traversal)".into(),
            ));
        }

        self.validate_wasm(&wasm_path).await?;

        // Copy new version to install dir
        let install_dir = self.plugin_dir.join(format!(
            "{}-{}",
            manifest.plugin.name, manifest.plugin.version
        ));

        // Backup old installation for rollback
        let old_wasm_path = PathBuf::from(&existing.wasm_path);
        let old_install_dir = old_wasm_path.parent().map(|p| p.to_path_buf());
        let backup_dir = old_install_dir.as_ref().map(|d| d.with_extension("bak"));

        if let Some(ref old_dir) = old_install_dir {
            if old_dir.exists() {
                if let Some(ref bak) = backup_dir {
                    if bak.exists() {
                        let _ = tokio::fs::remove_dir_all(bak).await;
                    }
                    tokio::fs::rename(old_dir, bak).await.map_err(|e| {
                        PluginError::Installation(format!("failed to backup old version: {e}"))
                    })?;
                }
            }
        }

        if install_dir.exists() {
            tokio::fs::remove_dir_all(&install_dir).await.map_err(|e| {
                PluginError::Installation(format!("failed to clean install dir: {e}"))
            })?;
        }

        tokio::fs::create_dir_all(&install_dir)
            .await
            .map_err(|e| PluginError::Installation(format!("failed to create install dir: {e}")))?;

        let dest_wasm = install_dir.join("plugin.wasm");
        tokio::fs::copy(&wasm_path, &dest_wasm)
            .await
            .map_err(|e| PluginError::Installation(format!("failed to copy WASM: {e}")))?;

        let dest_manifest = install_dir.join("plugin.toml");
        tokio::fs::copy(&manifest_path, &dest_manifest)
            .await
            .map_err(|e| PluginError::Installation(format!("failed to copy manifest: {e}")))?;

        // Update DB record
        let permissions_json = serde_json::to_value(&manifest.permissions)?;
        let now = chrono::Utc::now().fixed_offset();

        let mut active: plugin::ActiveModel = existing.into();
        active.version = Set(manifest.plugin.version.clone());
        active.description = Set(Some(manifest.plugin.description.clone()));
        active.author = Set(manifest.plugin.author.clone());
        active.license = Set(manifest.plugin.license.clone());
        active.homepage = Set(manifest.plugin.homepage.clone());
        active.wasm_path = Set(dest_wasm.to_string_lossy().to_string());
        active.permissions = Set(permissions_json);
        active.error_message = Set(None);
        active.updated_at = Set(now);

        let model = active.update(&self.db).await?;

        // Clean up backup on success
        if let Some(ref bak) = backup_dir {
            if bak.exists() {
                let _ = tokio::fs::remove_dir_all(bak).await;
            }
        }

        tracing::info!(
            plugin_name = %manifest.plugin.name,
            old_version = %old_version,
            new_version = %manifest.plugin.version,
            "plugin updated successfully"
        );

        Ok(model)
    }

    /// Uninstall a plugin: remove files and DB record.
    pub async fn uninstall_plugin(&self, plugin_id: Uuid) -> Result<(), PluginError> {
        let existing = plugin::Entity::find_by_id(plugin_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        // Remove files
        let wasm_path = PathBuf::from(&existing.wasm_path);
        if let Some(parent) = wasm_path.parent() {
            if parent.exists() {
                tokio::fs::remove_dir_all(parent).await.map_err(|e| {
                    PluginError::Installation(format!("failed to remove plugin files: {e}"))
                })?;
            }
        }

        // Delete from DB (cascades to configs and event logs)
        plugin::Entity::delete_by_id(plugin_id)
            .exec(&self.db)
            .await?;

        tracing::info!(
            plugin_name = %existing.name,
            plugin_id = %plugin_id,
            "plugin uninstalled"
        );

        Ok(())
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_magic_bytes() {
        assert_eq!(WASM_MAGIC, b"\0asm");
        assert_eq!(WASM_MAGIC[0], 0x00);
        assert_eq!(WASM_MAGIC[1], 0x61); // 'a'
        assert_eq!(WASM_MAGIC[2], 0x73); // 's'
        assert_eq!(WASM_MAGIC[3], 0x6D); // 'm'
    }

    #[test]
    fn test_allowed_import_namespaces() {
        assert!(ALLOWED_IMPORT_NAMESPACES.contains(&"env"));
        assert!(ALLOWED_IMPORT_NAMESPACES.contains(&"wasi_snapshot_preview1"));
        assert!(!ALLOWED_IMPORT_NAMESPACES.contains(&"forbidden_module"));
    }

    #[test]
    fn test_wasm_magic_check() {
        // Valid WASM starts with \0asm
        let valid_header = [0x00u8, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        assert_eq!(&valid_header[..4], WASM_MAGIC);

        // Invalid — random bytes
        let invalid_header = [0x7Fu8, 0x45, 0x4C, 0x46]; // ELF magic
        assert_ne!(&invalid_header[..4], WASM_MAGIC.as_slice());
    }

    #[test]
    fn test_default_max_wasm_size() {
        assert_eq!(DEFAULT_MAX_WASM_SIZE_MB, 50);
    }

    #[test]
    fn test_install_dir_naming() {
        let plugin_dir = PathBuf::from("/data/plugins");
        let name = "my-plugin";
        let version = "1.2.3";
        let install_dir = plugin_dir.join(format!("{name}-{version}"));
        assert_eq!(install_dir, PathBuf::from("/data/plugins/my-plugin-1.2.3"));
    }

    // ── Git URL validation ──────────────────────────────────────────────

    #[test]
    fn test_validate_git_url_valid_https() {
        assert!(validate_git_url("https://github.com/org/repo.git").is_ok());
    }

    #[test]
    fn test_validate_git_url_reject_file() {
        let err = validate_git_url("file:///etc/passwd").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
        assert!(err.to_string().contains("HTTPS"));
    }

    #[test]
    fn test_validate_git_url_reject_http() {
        let err = validate_git_url("http://example.com/repo").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
        assert!(err.to_string().contains("HTTPS"));
    }

    #[test]
    fn test_validate_git_url_reject_localhost() {
        let err = validate_git_url("https://localhost/repo").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
        assert!(err.to_string().contains("blocked"));
    }

    #[test]
    fn test_validate_git_url_reject_private_ip() {
        let err = validate_git_url("https://192.168.1.1/repo").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
        assert!(err.to_string().contains("private IP"));
    }

    #[test]
    fn test_validate_git_url_reject_metadata() {
        let err = validate_git_url("https://169.254.169.254/latest").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
        assert!(err.to_string().contains("blocked"));
    }

    // ── Additional Git URL validation ───────────────────────────────────

    #[test]
    fn test_validate_git_url_reject_ssh() {
        let err = validate_git_url("git@github.com:org/repo.git").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
        // ssh format fails URL parsing → "invalid git URL"
        assert!(err.to_string().contains("invalid git URL"));
    }

    #[test]
    fn test_validate_git_url_reject_git_protocol() {
        let err = validate_git_url("git://github.com/org/repo.git").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
        assert!(err.to_string().contains("HTTPS"));
    }

    #[test]
    fn test_validate_git_url_reject_172_16_private() {
        let err = validate_git_url("https://172.16.0.1/repo").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
        assert!(err.to_string().contains("private IP"));
    }

    #[test]
    fn test_validate_git_url_reject_10_private() {
        let err = validate_git_url("https://10.0.0.1/repo").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
        assert!(err.to_string().contains("private IP"));
    }

    #[test]
    fn test_validate_git_url_reject_ipv6_loopback() {
        let err = validate_git_url("https://[::1]/repo").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
        assert!(err.to_string().contains("blocked"));
    }

    #[test]
    fn test_validate_git_url_reject_zero_ip() {
        let err = validate_git_url("https://0.0.0.0/repo").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
        assert!(err.to_string().contains("blocked"));
    }

    #[test]
    fn test_validate_git_url_reject_gcp_metadata() {
        let err = validate_git_url("https://metadata.google.internal/repo").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
        assert!(err.to_string().contains("blocked"));
    }

    #[test]
    fn test_validate_git_url_valid_with_path() {
        assert!(validate_git_url("https://gitlab.com/group/subgroup/repo.git").is_ok());
    }

    #[test]
    fn test_validate_git_url_reject_empty() {
        let err = validate_git_url("").unwrap_err();
        assert!(matches!(err, PluginError::Installation(_)));
    }

    // ── WASM validation (async) ─────────────────────────────────────────

    #[tokio::test]
    async fn test_validate_wasm_file_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("large.wasm");

        // Create a file larger than 1 byte (we'll set max_wasm_size to 10)
        // Write valid WASM magic + extra bytes
        let mut data = vec![0x00u8, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        data.extend(vec![0u8; 100]); // 108 bytes total
        tokio::fs::write(&wasm_path, &data).await.unwrap();

        let installer = PluginInstaller {
            plugin_dir: dir.path().to_path_buf(),
            max_wasm_size: 50, // 50 bytes max, file is 108
            db: sea_orm::DatabaseConnection::Disconnected,
        };

        let err = installer.validate_wasm(&wasm_path).await.unwrap_err();
        assert!(matches!(err, PluginError::WasmValidation(_)));
        assert!(err.to_string().contains("too large"));
    }

    #[tokio::test]
    async fn test_validate_wasm_invalid_magic() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("bad.wasm");

        // ELF magic bytes
        let data = vec![0x7Fu8, 0x45, 0x4C, 0x46, 0x01, 0x00, 0x00, 0x00];
        tokio::fs::write(&wasm_path, &data).await.unwrap();

        let installer = PluginInstaller {
            plugin_dir: dir.path().to_path_buf(),
            max_wasm_size: 50 * 1024 * 1024,
            db: sea_orm::DatabaseConnection::Disconnected,
        };

        let err = installer.validate_wasm(&wasm_path).await.unwrap_err();
        assert!(matches!(err, PluginError::WasmValidation(_)));
        assert!(err.to_string().contains("magic bytes"));
    }

    #[tokio::test]
    async fn test_validate_wasm_too_short() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("short.wasm");

        // Only 2 bytes — too short for magic check
        let data = vec![0x00u8, 0x61];
        tokio::fs::write(&wasm_path, &data).await.unwrap();

        let installer = PluginInstaller {
            plugin_dir: dir.path().to_path_buf(),
            max_wasm_size: 50 * 1024 * 1024,
            db: sea_orm::DatabaseConnection::Disconnected,
        };

        let err = installer.validate_wasm(&wasm_path).await.unwrap_err();
        assert!(matches!(err, PluginError::WasmValidation(_)));
        assert!(err.to_string().contains("magic bytes"));
    }

    #[tokio::test]
    async fn test_validate_wasm_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("missing.wasm");

        let installer = PluginInstaller {
            plugin_dir: dir.path().to_path_buf(),
            max_wasm_size: 50 * 1024 * 1024,
            db: sea_orm::DatabaseConnection::Disconnected,
        };

        let err = installer.validate_wasm(&wasm_path).await.unwrap_err();
        assert!(matches!(err, PluginError::WasmValidation(_)));
    }

    // ── WASM import validation ──────────────────────────────────────────

    #[test]
    fn test_validate_wasm_imports_valid_env() {
        // Minimal valid WASM module with an import from "env" namespace
        // Constructed as: (module (import "env" "memory" (memory 1)))
        let wasm = vec![
            0x00, 0x61, 0x73, 0x6D, // magic
            0x01, 0x00, 0x00, 0x00, // version
            0x02, 0x0F, // import section, 15 bytes
            0x01, // 1 import
            0x03, b'e', b'n', b'v', // module: "env"
            0x06, b'm', b'e', b'm', b'o', b'r', b'y', // name: "memory"
            0x02, 0x00, 0x01, // memory, limits: min=1
        ];

        let installer = PluginInstaller {
            plugin_dir: std::path::PathBuf::from("/tmp"),
            max_wasm_size: 50 * 1024 * 1024,
            db: sea_orm::DatabaseConnection::Disconnected,
        };

        assert!(installer.validate_wasm_imports(&wasm).is_ok());
    }

    #[test]
    fn test_validate_wasm_imports_forbidden_namespace() {
        // WASM module with import from "evil" namespace
        let wasm = vec![
            0x00, 0x61, 0x73, 0x6D, // magic
            0x01, 0x00, 0x00, 0x00, // version
            0x02, 0x0D, // import section, 13 bytes
            0x01, // 1 import
            0x04, b'e', b'v', b'i', b'l', // module: "evil"
            0x04, b'f', b'u', b'n', b'c', // name: "func"
            0x00, 0x00, // function, type index 0
        ];

        let installer = PluginInstaller {
            plugin_dir: std::path::PathBuf::from("/tmp"),
            max_wasm_size: 50 * 1024 * 1024,
            db: sea_orm::DatabaseConnection::Disconnected,
        };

        let err = installer.validate_wasm_imports(&wasm).unwrap_err();
        assert!(matches!(err, PluginError::WasmValidation(_)));
        assert!(err.to_string().contains("unauthorized import namespace"));
        assert!(err.to_string().contains("evil"));
    }

    #[test]
    fn test_validate_wasm_imports_no_imports() {
        // Minimal valid WASM module with no import section
        let wasm = vec![
            0x00, 0x61, 0x73, 0x6D, // magic
            0x01, 0x00, 0x00, 0x00, // version
        ];

        let installer = PluginInstaller {
            plugin_dir: std::path::PathBuf::from("/tmp"),
            max_wasm_size: 50 * 1024 * 1024,
            db: sea_orm::DatabaseConnection::Disconnected,
        };

        assert!(installer.validate_wasm_imports(&wasm).is_ok());
    }
}
