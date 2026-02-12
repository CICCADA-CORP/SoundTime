//! Theme management API endpoints.
//!
//! Admin endpoints for installing, enabling, disabling, updating, and
//! uninstalling CSS themes. Public endpoints serve the active theme CSS
//! and static assets.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use regex::Regex;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use serde_json::json;
use soundtime_db::entities::{instance_setting, theme};
use soundtime_db::AppState;
use std::path::{Path as StdPath, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;

// ─── Constants ──────────────────────────────────────────────────────────

/// Allowed file extensions for theme packages.
const ALLOWED_EXTENSIONS: &[&str] = &[
    "css", "png", "jpg", "jpeg", "svg", "webp", "woff2", "woff", "ttf", "otf",
];

/// Default max theme size: 20 MB.
const DEFAULT_MAX_THEME_SIZE_MB: u64 = 20;

/// Theme name regex: lowercase letters, digits, hyphens, 2-64 chars.
const THEME_NAME_PATTERN: &str = r"^[a-z][a-z0-9-]{1,63}$";

// ─── Theme Manifest ─────────────────────────────────────────────────────

/// Parsed theme.toml manifest.
#[derive(Debug, Deserialize)]
struct ThemeManifest {
    theme: ThemeInfo,
    assets: ThemeAssets,
}

#[derive(Debug, Deserialize)]
struct ThemeInfo {
    name: String,
    version: String,
    description: Option<String>,
    author: Option<String>,
    license: Option<String>,
    homepage: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ThemeAssets {
    css: String,
    assets_dir: Option<String>,
}

impl ThemeManifest {
    /// Parse and validate a `theme.toml` manifest.
    fn parse_and_validate(content: &str) -> Result<Self, String> {
        let manifest: ThemeManifest =
            toml::from_str(content).map_err(|e| format!("invalid theme.toml: {e}"))?;

        // Validate name against regex
        let name_re =
            Regex::new(THEME_NAME_PATTERN).map_err(|e| format!("invalid name regex: {e}"))?;
        if !name_re.is_match(&manifest.theme.name) {
            return Err(format!(
                "invalid theme name '{}': must match pattern {} (lowercase letters, digits, hyphens, 2-64 chars)",
                manifest.theme.name, THEME_NAME_PATTERN
            ));
        }

        // Validate version is semver-ish (contains a dot)
        if !manifest.theme.version.contains('.') {
            return Err(format!(
                "invalid theme version '{}': must be semver-like (e.g. 1.0.0)",
                manifest.theme.version
            ));
        }

        // Validate CSS path ends in .css and has no path traversal
        if !manifest.assets.css.ends_with(".css") {
            return Err(format!(
                "CSS path '{}' must end with .css",
                manifest.assets.css
            ));
        }
        if manifest.assets.css.contains("..") || manifest.assets.css.starts_with('/') {
            return Err(format!(
                "CSS path '{}' must be relative with no '..' components",
                manifest.assets.css
            ));
        }

        // Validate assets_dir (if set) has no path traversal
        if let Some(ref assets_dir) = manifest.assets.assets_dir {
            if assets_dir.contains("..") || assets_dir.starts_with('/') {
                return Err(format!(
                    "assets_dir '{}' must be relative with no '..' components",
                    assets_dir
                ));
            }
        }

        Ok(manifest)
    }
}

// ─── Security helpers ───────────────────────────────────────────────────

/// Validate a git URL for security.
///
/// Only HTTPS URLs are allowed. File, HTTP, SSH, and git protocols are
/// blocked to prevent SSRF and local file access.
fn validate_git_url(url: &str) -> Result<(), String> {
    let parsed =
        url::Url::parse(url).map_err(|_| format!("invalid git URL: '{url}'"))?;

    if parsed.scheme() != "https" {
        return Err(format!(
            "only HTTPS git URLs are allowed, got scheme '{}' in '{url}'",
            parsed.scheme()
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| format!("git URL has no host: '{url}'"))?;

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
        return Err(format!(
            "git URL host '{host}' is blocked (private/reserved address)"
        ));
    }

    // Block 10.x.x.x, 172.16-31.x.x, 192.168.x.x
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let is_private = match ip {
            std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
            std::net::IpAddr::V6(v6) => v6.is_loopback(),
        };
        if is_private {
            return Err(format!(
                "git URL resolves to private IP: '{host}'"
            ));
        }
    }

    Ok(())
}

/// Check whether a file extension is in the allowed list.
fn is_allowed_extension(path: &StdPath) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| ALLOWED_EXTENSIONS.contains(&e))
        .unwrap_or(false)
}

/// Walk a directory and ensure all files have allowed extensions.
/// Rejects any .js, .html, or other disallowed files.
async fn validate_theme_files(dir: &StdPath) -> Result<(), String> {
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&current)
            .await
            .map_err(|e| format!("failed to read directory {}: {e}", current.display()))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| format!("failed to read dir entry: {e}"))?
        {
            let path = entry.path();
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            // Skip .git directory
            if file_name_str == ".git" {
                continue;
            }

            let file_type = entry
                .file_type()
                .await
                .map_err(|e| format!("failed to read file type for {}: {e}", path.display()))?;

            // SECURITY: reject symlinks
            if file_type.is_symlink() {
                return Err(format!(
                    "symlinks are not allowed in theme packages: {}",
                    path.display()
                ));
            }

            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                // Allow theme.toml at root
                if file_name_str == "theme.toml" {
                    continue;
                }
                if !is_allowed_extension(&path) {
                    return Err(format!(
                        "disallowed file in theme package: {} (allowed extensions: {:?})",
                        path.display(),
                        ALLOWED_EXTENSIONS
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Recursively copy only files with allowed extensions from src to dest.
/// Returns total bytes copied.
async fn copy_filtered_dir(src: &StdPath, dest: &StdPath) -> Result<u64, String> {
    tokio::fs::create_dir_all(dest)
        .await
        .map_err(|e| format!("failed to create dir {}: {e}", dest.display()))?;

    let mut total_bytes: u64 = 0;
    let mut stack = vec![(src.to_path_buf(), dest.to_path_buf())];

    while let Some((src_dir, dest_dir)) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&src_dir)
            .await
            .map_err(|e| format!("failed to read dir {}: {e}", src_dir.display()))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| format!("failed to read dir entry: {e}"))?
        {
            let path = entry.path();
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            // Skip .git directory
            if file_name_str == ".git" {
                continue;
            }

            let file_type = entry
                .file_type()
                .await
                .map_err(|e| format!("failed to read file type: {e}"))?;

            // SECURITY: reject symlinks — check metadata without following
            if file_type.is_symlink() {
                continue;
            }

            let dest_path = dest_dir.join(&file_name);

            if file_type.is_dir() {
                tokio::fs::create_dir_all(&dest_path)
                    .await
                    .map_err(|e| format!("failed to create dir {}: {e}", dest_path.display()))?;
                stack.push((path, dest_path));
            } else if file_type.is_file() && is_allowed_extension(&path) {
                let bytes = tokio::fs::copy(&path, &dest_path)
                    .await
                    .map_err(|e| format!("failed to copy {}: {e}", path.display()))?;
                total_bytes += bytes;
            }
        }
    }

    Ok(total_bytes)
}

/// Calculate total size of a directory tree in bytes.
async fn dir_size(dir: &StdPath) -> Result<u64, String> {
    let mut total: u64 = 0;
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&current)
            .await
            .map_err(|e| format!("failed to read dir {}: {e}", current.display()))?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| format!("failed to read dir entry: {e}"))?
        {
            let path = entry.path();
            let file_type = entry
                .file_type()
                .await
                .map_err(|e| format!("failed to read file type: {e}"))?;

            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                let meta = entry
                    .metadata()
                    .await
                    .map_err(|e| format!("failed to read metadata: {e}"))?;
                total += meta.len();
            }
        }
    }

    Ok(total)
}

// ─── Installer ──────────────────────────────────────────────────────────

/// Theme installer handles the complete installation flow.
pub struct ThemeInstaller {
    /// Base directory for installed themes.
    theme_dir: PathBuf,
    /// Maximum theme size in bytes.
    max_theme_size: u64,
    /// Database connection.
    db: sea_orm::DatabaseConnection,
}

impl ThemeInstaller {
    /// Create a new installer with configuration from environment.
    pub fn new(db: sea_orm::DatabaseConnection) -> Self {
        let theme_dir =
            std::env::var("THEME_DIR").unwrap_or_else(|_| "/data/themes".to_string());
        let max_theme_size = std::env::var("THEME_MAX_SIZE_MB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MAX_THEME_SIZE_MB)
            * 1024
            * 1024;

        Self {
            theme_dir: PathBuf::from(theme_dir),
            max_theme_size,
            db,
        }
    }

    /// Install a theme from a git repository URL.
    ///
    /// The full installation flow:
    /// 1. Validate git URL
    /// 2. Clone repo to temp directory
    /// 3. Parse and validate `theme.toml`
    /// 4. Validate all files (no JS/HTML)
    /// 5. Check total size against limit
    /// 6. Check name uniqueness in DB
    /// 7. Copy CSS + filtered assets to install dir
    /// 8. Insert DB record with status "disabled"
    pub async fn install_from_git(
        &self,
        git_url: &str,
        installed_by: Option<Uuid>,
    ) -> Result<theme::Model, String> {
        tracing::info!(git_url = %git_url, "starting theme installation");

        // SECURITY: Validate git URL before cloning
        validate_git_url(git_url)?;

        // 1. Clone to temp directory
        let temp_dir = tempfile::tempdir()
            .map_err(|e| format!("failed to create temp dir: {e}"))?;

        self.clone_repo(git_url, temp_dir.path())?;

        // 2. Parse and validate manifest
        let manifest_path = temp_dir.path().join("theme.toml");
        if !manifest_path.exists() {
            return Err("theme.toml not found in repository root".into());
        }

        let manifest_content = tokio::fs::read_to_string(&manifest_path)
            .await
            .map_err(|e| format!("failed to read theme.toml: {e}"))?;

        let manifest = ThemeManifest::parse_and_validate(&manifest_content)?;

        // 3. Validate all files (no JS/HTML)
        validate_theme_files(temp_dir.path()).await?;

        // 4. Check total size
        let total_size = dir_size(temp_dir.path()).await?;
        if total_size > self.max_theme_size {
            return Err(format!(
                "theme package too large: {} bytes (max: {} bytes)",
                total_size, self.max_theme_size
            ));
        }

        // 5. Verify CSS file exists
        let css_src = temp_dir.path().join(&manifest.assets.css);
        if !css_src.exists() {
            return Err(format!(
                "CSS file not found at declared path: {}",
                manifest.assets.css
            ));
        }

        // SECURITY: Canonicalize and verify path stays within temp dir
        let css_src = css_src
            .canonicalize()
            .map_err(|e| format!("invalid CSS path: {e}"))?;
        if !css_src.starts_with(temp_dir.path()) {
            return Err("CSS path escapes repository directory (path traversal)".into());
        }

        // 6. Check name uniqueness in DB
        let existing = theme::Entity::find()
            .filter(theme::Column::Name.eq(&manifest.theme.name))
            .one(&self.db)
            .await
            .map_err(|e| format!("database error: {e}"))?;

        if existing.is_some() {
            return Err(format!(
                "theme '{}' is already installed",
                manifest.theme.name
            ));
        }

        // 7. Copy to install directory
        let install_dir = self.theme_dir.join(format!(
            "{}-{}",
            manifest.theme.name, manifest.theme.version
        ));

        if install_dir.exists() {
            tokio::fs::remove_dir_all(&install_dir)
                .await
                .map_err(|e| format!("failed to clean install dir: {e}"))?;
        }

        tokio::fs::create_dir_all(&install_dir)
            .await
            .map_err(|e| format!("failed to create install dir: {e}"))?;

        // Copy CSS file
        let dest_css = install_dir.join("theme.css");
        tokio::fs::copy(&css_src, &dest_css)
            .await
            .map_err(|e| format!("failed to copy CSS file: {e}"))?;

        // Copy assets if applicable
        let dest_assets_path = if let Some(ref assets_dir) = manifest.assets.assets_dir {
            let assets_src = temp_dir.path().join(assets_dir);
            if assets_src.exists() {
                // SECURITY: Canonicalize and verify path stays within temp dir
                let assets_src = assets_src
                    .canonicalize()
                    .map_err(|e| format!("invalid assets path: {e}"))?;
                if !assets_src.starts_with(temp_dir.path()) {
                    return Err("assets_dir path escapes repository directory (path traversal)".into());
                }

                let dest_assets = install_dir.join("assets");
                copy_filtered_dir(&assets_src, &dest_assets).await?;
                Some(dest_assets.to_string_lossy().to_string())
            } else {
                None
            }
        } else {
            None
        };

        // 8. Register in database
        let now = chrono::Utc::now().fixed_offset();
        let theme_id = Uuid::new_v4();

        let new_theme = theme::ActiveModel {
            id: Set(theme_id),
            name: Set(manifest.theme.name.clone()),
            version: Set(manifest.theme.version.clone()),
            description: Set(manifest.theme.description.clone()),
            author: Set(manifest.theme.author.clone()),
            license: Set(manifest.theme.license.clone()),
            homepage: Set(manifest.theme.homepage.clone()),
            git_url: Set(git_url.to_string()),
            css_path: Set(dest_css.to_string_lossy().to_string()),
            assets_path: Set(dest_assets_path),
            status: Set("disabled".to_string()),
            installed_at: Set(now),
            updated_at: Set(now),
            installed_by: Set(installed_by),
        };

        let model = new_theme.insert(&self.db).await.map_err(|e| {
            format!("failed to insert theme record: {e}")
        })?;

        tracing::info!(
            theme_name = %manifest.theme.name,
            theme_id = %theme_id,
            version = %manifest.theme.version,
            "theme installed successfully"
        );

        Ok(model)
    }

    /// Update an installed theme by re-cloning and re-validating.
    pub async fn update_theme(&self, theme_id: Uuid) -> Result<theme::Model, String> {
        let existing = theme::Entity::find_by_id(theme_id)
            .one(&self.db)
            .await
            .map_err(|e| format!("database error: {e}"))?
            .ok_or_else(|| format!("theme not found: {theme_id}"))?;

        let old_version = existing.version.clone();
        let git_url = existing.git_url.clone();

        tracing::info!(
            theme_name = %existing.name,
            old_version = %old_version,
            "starting theme update"
        );

        // SECURITY: Validate git URL before cloning
        validate_git_url(&git_url)?;

        // Clone new version to temp
        let temp_dir = tempfile::tempdir()
            .map_err(|e| format!("failed to create temp dir: {e}"))?;

        self.clone_repo(&git_url, temp_dir.path())?;

        // Parse and validate new manifest
        let manifest_path = temp_dir.path().join("theme.toml");
        if !manifest_path.exists() {
            return Err("theme.toml not found in updated repository".into());
        }

        let manifest_content = tokio::fs::read_to_string(&manifest_path)
            .await
            .map_err(|e| format!("failed to read theme.toml: {e}"))?;

        let manifest = ThemeManifest::parse_and_validate(&manifest_content)?;

        // Verify theme name matches
        if manifest.theme.name != existing.name {
            return Err(format!(
                "theme name mismatch: expected '{}', got '{}'",
                existing.name, manifest.theme.name
            ));
        }

        // Validate all files
        validate_theme_files(temp_dir.path()).await?;

        // Check size
        let total_size = dir_size(temp_dir.path()).await?;
        if total_size > self.max_theme_size {
            return Err(format!(
                "theme package too large: {} bytes (max: {} bytes)",
                total_size, self.max_theme_size
            ));
        }

        // Verify CSS file exists
        let css_src = temp_dir.path().join(&manifest.assets.css);
        if !css_src.exists() {
            return Err(format!(
                "CSS file not found at declared path: {}",
                manifest.assets.css
            ));
        }

        // SECURITY: Canonicalize and verify path stays within temp dir
        let css_src = css_src
            .canonicalize()
            .map_err(|e| format!("invalid CSS path: {e}"))?;
        if !css_src.starts_with(temp_dir.path()) {
            return Err("CSS path escapes repository directory (path traversal)".into());
        }

        // Set up new install directory
        let install_dir = self.theme_dir.join(format!(
            "{}-{}",
            manifest.theme.name, manifest.theme.version
        ));

        // Backup old installation
        let old_css_path = PathBuf::from(&existing.css_path);
        let old_install_dir = old_css_path.parent().map(|p| p.to_path_buf());
        let backup_dir = old_install_dir.as_ref().map(|d| d.with_extension("bak"));

        if let Some(ref old_dir) = old_install_dir {
            if old_dir.exists() {
                if let Some(ref bak) = backup_dir {
                    if bak.exists() {
                        let _ = tokio::fs::remove_dir_all(bak).await;
                    }
                    tokio::fs::rename(old_dir, bak)
                        .await
                        .map_err(|e| format!("failed to backup old version: {e}"))?;
                }
            }
        }

        if install_dir.exists() {
            tokio::fs::remove_dir_all(&install_dir)
                .await
                .map_err(|e| format!("failed to clean install dir: {e}"))?;
        }

        tokio::fs::create_dir_all(&install_dir)
            .await
            .map_err(|e| format!("failed to create install dir: {e}"))?;

        // Copy CSS
        let dest_css = install_dir.join("theme.css");
        tokio::fs::copy(&css_src, &dest_css)
            .await
            .map_err(|e| format!("failed to copy CSS: {e}"))?;

        // Copy assets
        let dest_assets_path = if let Some(ref assets_dir) = manifest.assets.assets_dir {
            let assets_src = temp_dir.path().join(assets_dir);
            if assets_src.exists() {
                let assets_src = assets_src
                    .canonicalize()
                    .map_err(|e| format!("invalid assets path: {e}"))?;
                if !assets_src.starts_with(temp_dir.path()) {
                    return Err("assets_dir path escapes repository directory (path traversal)".into());
                }
                let dest_assets = install_dir.join("assets");
                copy_filtered_dir(&assets_src, &dest_assets).await?;
                Some(dest_assets.to_string_lossy().to_string())
            } else {
                None
            }
        } else {
            None
        };

        // Update DB record
        let now = chrono::Utc::now().fixed_offset();

        let mut active: theme::ActiveModel = existing.into();
        active.version = Set(manifest.theme.version.clone());
        active.description = Set(manifest.theme.description.clone());
        active.author = Set(manifest.theme.author.clone());
        active.license = Set(manifest.theme.license.clone());
        active.homepage = Set(manifest.theme.homepage.clone());
        active.css_path = Set(dest_css.to_string_lossy().to_string());
        active.assets_path = Set(dest_assets_path);
        active.updated_at = Set(now);

        let model = active.update(&self.db).await.map_err(|e| {
            format!("failed to update theme record: {e}")
        })?;

        // Clean up backup on success
        if let Some(ref bak) = backup_dir {
            if bak.exists() {
                let _ = tokio::fs::remove_dir_all(bak).await;
            }
        }

        tracing::info!(
            theme_name = %manifest.theme.name,
            old_version = %old_version,
            new_version = %manifest.theme.version,
            "theme updated successfully"
        );

        Ok(model)
    }

    /// Uninstall a theme: remove files and DB record.
    ///
    /// Also clears `active_theme_id` from instance_settings if this was the active theme.
    pub async fn uninstall_theme(&self, theme_id: Uuid) -> Result<(), String> {
        let existing = theme::Entity::find_by_id(theme_id)
            .one(&self.db)
            .await
            .map_err(|e| format!("database error: {e}"))?
            .ok_or_else(|| format!("theme not found: {theme_id}"))?;

        // Remove files
        let css_path = PathBuf::from(&existing.css_path);
        if let Some(parent) = css_path.parent() {
            if parent.exists() {
                tokio::fs::remove_dir_all(parent)
                    .await
                    .map_err(|e| format!("failed to remove theme files: {e}"))?;
            }
        }

        // Clear active_theme_id if this was the active theme
        let active_setting = instance_setting::Entity::find()
            .filter(instance_setting::Column::Key.eq("active_theme_id"))
            .one(&self.db)
            .await
            .map_err(|e| format!("database error: {e}"))?;

        if let Some(setting) = active_setting {
            if setting.value == theme_id.to_string() {
                instance_setting::Entity::delete_by_id(setting.id)
                    .exec(&self.db)
                    .await
                    .map_err(|e| format!("failed to clear active_theme_id: {e}"))?;
            }
        }

        // Delete from DB
        theme::Entity::delete_by_id(theme_id)
            .exec(&self.db)
            .await
            .map_err(|e| format!("failed to delete theme record: {e}"))?;

        tracing::info!(
            theme_name = %existing.name,
            theme_id = %theme_id,
            "theme uninstalled"
        );

        Ok(())
    }

    /// Clone a git repository to the given directory.
    fn clone_repo(&self, url: &str, dest: &StdPath) -> Result<(), String> {
        tracing::info!(url = %url, dest = %dest.display(), "cloning theme repository");

        git2::Repository::clone(url, dest)
            .map_err(|e| format!("git clone failed: {e}"))?;

        Ok(())
    }
}

// ─── Request / Response types ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct InstallThemeRequest {
    pub git_url: String,
}

#[derive(Debug, Serialize)]
pub struct ThemeListResponse {
    pub themes: Vec<theme::Model>,
}

#[derive(Debug, Serialize)]
pub struct ActiveThemeResponse {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub css_url: String,
}

// ─── Admin Handlers ─────────────────────────────────────────────────────

/// GET /api/admin/themes — List all installed themes.
pub async fn list_themes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ThemeListResponse>, (StatusCode, Json<serde_json::Value>)> {
    let themes = theme::Entity::find().all(&state.db).await.map_err(|e| {
        tracing::error!("failed to list themes: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("DB error: {e}") })),
        )
    })?;

    Ok(Json(ThemeListResponse { themes }))
}

/// POST /api/admin/themes/install — Install a theme from a git repository.
pub async fn install_theme(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<AuthUser>,
    Json(body): Json<InstallThemeRequest>,
) -> Result<(StatusCode, Json<theme::Model>), (StatusCode, Json<serde_json::Value>)> {
    let installer = ThemeInstaller::new(state.db.clone());

    let model = installer
        .install_from_git(&body.git_url, Some(user.0.sub))
        .await
        .map_err(|e| {
            tracing::error!(git_url = %body.git_url, "theme installation failed: {e}");
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{e}") })),
            )
        })?;

    tracing::info!(
        theme_name = %model.name,
        theme_id = %model.id,
        "theme installed via API"
    );

    Ok((StatusCode::CREATED, Json(model)))
}

/// POST /api/admin/themes/:id/enable — Enable a theme and set it as active.
///
/// Disables any previously enabled theme, sets the new theme to "enabled",
/// and upserts `active_theme_id` in instance_settings.
pub async fn enable_theme(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Verify theme exists
    let theme_model = theme::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(theme_id = %id, "failed to query theme: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("DB error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Theme not found" })),
            )
        })?;

    let now = chrono::Utc::now().fixed_offset();

    // 1. Disable any previously enabled theme
    let enabled_themes = theme::Entity::find()
        .filter(theme::Column::Status.eq("enabled"))
        .all(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("failed to query enabled themes: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    for t in enabled_themes {
        let mut active: theme::ActiveModel = t.into();
        active.status = Set("disabled".to_string());
        active.updated_at = Set(now);
        active.update(&state.db).await.map_err(|e| {
            tracing::error!("failed to disable previous theme: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("DB error: {e}") })),
            )
        })?;
    }

    // 2. Set the new theme to "enabled"
    let mut active: theme::ActiveModel = theme_model.into();
    active.status = Set("enabled".to_string());
    active.updated_at = Set(now);
    active.update(&state.db).await.map_err(|e| {
        tracing::error!(theme_id = %id, "failed to enable theme: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("DB error: {e}") })),
        )
    })?;

    // 3. Upsert active_theme_id in instance_settings
    let existing_setting = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("active_theme_id"))
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("failed to query instance_settings: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    if let Some(setting) = existing_setting {
        let mut active_setting: instance_setting::ActiveModel = setting.into();
        active_setting.value = Set(id.to_string());
        active_setting.updated_at = Set(now);
        active_setting.update(&state.db).await.map_err(|e| {
            tracing::error!("failed to update active_theme_id setting: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("DB error: {e}") })),
            )
        })?;
    } else {
        let new_setting = instance_setting::ActiveModel {
            id: Set(Uuid::new_v4()),
            key: Set("active_theme_id".to_string()),
            value: Set(id.to_string()),
            updated_at: Set(now),
        };
        new_setting.insert(&state.db).await.map_err(|e| {
            tracing::error!("failed to insert active_theme_id setting: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("DB error: {e}") })),
            )
        })?;
    }

    tracing::info!(theme_id = %id, "theme enabled via API");
    Ok(Json(json!({ "status": "enabled" })))
}

/// POST /api/admin/themes/:id/disable — Disable a theme.
///
/// Sets the theme status to "disabled" and removes `active_theme_id`
/// from instance_settings.
pub async fn disable_theme(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Verify theme exists
    let theme_model = theme::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(theme_id = %id, "failed to query theme: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("DB error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Theme not found" })),
            )
        })?;

    let now = chrono::Utc::now().fixed_offset();

    // 1. Set theme status to "disabled"
    let mut active: theme::ActiveModel = theme_model.into();
    active.status = Set("disabled".to_string());
    active.updated_at = Set(now);
    active.update(&state.db).await.map_err(|e| {
        tracing::error!(theme_id = %id, "failed to disable theme: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("DB error: {e}") })),
        )
    })?;

    // 2. Delete active_theme_id from instance_settings
    let setting = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("active_theme_id"))
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("failed to query instance_settings: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    if let Some(setting) = setting {
        instance_setting::Entity::delete_by_id(setting.id)
            .exec(&state.db)
            .await
            .map_err(|e| {
                tracing::error!("failed to delete active_theme_id setting: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": format!("DB error: {e}") })),
                )
            })?;
    }

    tracing::info!(theme_id = %id, "theme disabled via API");
    Ok(Json(json!({ "status": "disabled" })))
}

/// POST /api/admin/themes/:id/update — Update a theme from its git repo.
pub async fn update_theme(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<theme::Model>, (StatusCode, Json<serde_json::Value>)> {
    let installer = ThemeInstaller::new(state.db.clone());

    let model = installer.update_theme(id).await.map_err(|e| {
        tracing::error!(theme_id = %id, "failed to update theme: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e}") })),
        )
    })?;

    tracing::info!(
        theme_id = %id,
        new_version = %model.version,
        "theme updated via API"
    );

    Ok(Json(model))
}

/// DELETE /api/admin/themes/:id — Uninstall a theme.
pub async fn uninstall_theme(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let installer = ThemeInstaller::new(state.db.clone());

    installer.uninstall_theme(id).await.map_err(|e| {
        tracing::error!(theme_id = %id, "failed to uninstall theme: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e}") })),
        )
    })?;

    tracing::info!(theme_id = %id, "theme uninstalled via API");
    Ok(StatusCode::NO_CONTENT)
}

// ─── Public Handlers ────────────────────────────────────────────────────

/// Look up the currently active theme from instance_settings + themes table.
async fn load_active_theme(
    db: &sea_orm::DatabaseConnection,
) -> Result<Option<theme::Model>, (StatusCode, Json<serde_json::Value>)> {
    let setting = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("active_theme_id"))
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!("failed to query active_theme_id setting: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    let setting = match setting {
        Some(s) => s,
        None => return Ok(None),
    };

    let theme_id: Uuid = setting.value.parse().map_err(|_| {
        tracing::error!(value = %setting.value, "invalid UUID in active_theme_id setting");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "invalid active_theme_id setting" })),
        )
    })?;

    let theme_model = theme::Entity::find_by_id(theme_id)
        .one(db)
        .await
        .map_err(|e| {
            tracing::error!(theme_id = %theme_id, "failed to query active theme: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    Ok(theme_model)
}

/// GET /api/themes/active — Get the currently active theme metadata.
///
/// Returns `ActiveThemeResponse` or 204 No Content if no theme is active.
pub async fn get_active_theme(
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    use axum::response::IntoResponse;

    let theme_model = load_active_theme(&state.db).await?;

    match theme_model {
        Some(t) => {
            let response = ActiveThemeResponse {
                id: t.id.to_string(),
                name: t.name,
                version: t.version,
                description: t.description,
                author: t.author,
                css_url: "/api/themes/active.css".to_string(),
            };
            Ok(Json(response).into_response())
        }
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

/// GET /api/themes/active.css — Serve the active theme's CSS file.
///
/// Returns the CSS with `Content-Type: text/css` and `Cache-Control: public, max-age=3600`.
/// Returns 204 No Content if no theme is active.
pub async fn serve_active_css(
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    use axum::response::IntoResponse;

    let theme_model = load_active_theme(&state.db).await?;

    let theme_model = match theme_model {
        Some(t) => t,
        None => return Ok(StatusCode::NO_CONTENT.into_response()),
    };

    let css_path = PathBuf::from(&theme_model.css_path);
    if !css_path.exists() {
        tracing::error!(
            theme_id = %theme_model.id,
            css_path = %theme_model.css_path,
            "active theme CSS file not found on disk"
        );
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Theme CSS file not found" })),
        ));
    }

    let data = tokio::fs::read(&css_path).await.map_err(|e| {
        tracing::error!(theme_id = %theme_model.id, "failed to read CSS file: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to read CSS file" })),
        )
    })?;

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        "text/css".parse().unwrap(),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        "public, max-age=3600".parse().unwrap(),
    );

    Ok((headers, data).into_response())
}

/// GET /api/themes/assets/{*path} — Serve static assets from the active theme.
///
/// Path traversal is prevented by canonicalization + starts_with check.
pub async fn serve_theme_asset(
    State(state): State<Arc<AppState>>,
    Path(file_path): Path<String>,
) -> Result<impl axum::response::IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let theme_model = load_active_theme(&state.db).await?;

    let theme_model = match theme_model {
        Some(t) => t,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "No active theme" })),
            ))
        }
    };

    let assets_path = match &theme_model.assets_path {
        Some(p) => PathBuf::from(p),
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Active theme has no assets" })),
            ))
        }
    };

    if !assets_path.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Theme assets directory not found" })),
        ));
    }

    // SECURITY: prevent path traversal
    let requested = assets_path.join(&file_path);
    let canonical = requested.canonicalize().map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "File not found" })),
        )
    })?;
    let canonical_assets_dir = assets_path.canonicalize().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Theme assets directory error" })),
        )
    })?;

    if !canonical.starts_with(&canonical_assets_dir) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Path traversal detected" })),
        ));
    }

    if !canonical.is_file() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "File not found" })),
        ));
    }

    // Read file
    let data = tokio::fs::read(&canonical).await.map_err(|e| {
        tracing::error!(path = %file_path, "failed to read theme asset: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to read file" })),
        )
    })?;

    // Determine content type from extension
    let content_type = match canonical.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    };

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        content_type.parse().unwrap(),
    );
    headers.insert(
        axum::http::header::CACHE_CONTROL,
        "public, max-age=3600".parse().unwrap(),
    );

    Ok((headers, data))
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Manifest parsing ────────────────────────────────────────────────

    /// Valid manifest parses correctly.
    #[test]
    fn test_parse_valid_manifest() {
        let toml_str = r#"
[theme]
name = "dark-mode"
version = "1.0.0"
description = "A dark theme"
author = "Test Author"
license = "MIT"

[assets]
css = "theme.css"
assets_dir = "assets"
"#;
        let manifest = ThemeManifest::parse_and_validate(toml_str).unwrap();
        assert_eq!(manifest.theme.name, "dark-mode");
        assert_eq!(manifest.theme.version, "1.0.0");
        assert_eq!(manifest.assets.css, "theme.css");
        assert_eq!(manifest.assets.assets_dir, Some("assets".to_string()));
    }

    /// Manifest with invalid name is rejected.
    #[test]
    fn test_parse_manifest_invalid_name() {
        let toml_str = r#"
[theme]
name = "INVALID"
version = "1.0.0"

[assets]
css = "theme.css"
"#;
        let err = ThemeManifest::parse_and_validate(toml_str).unwrap_err();
        assert!(err.contains("invalid theme name"));
    }

    /// Manifest with non-semver version is rejected.
    #[test]
    fn test_parse_manifest_invalid_version() {
        let toml_str = r#"
[theme]
name = "valid-name"
version = "1"

[assets]
css = "theme.css"
"#;
        let err = ThemeManifest::parse_and_validate(toml_str).unwrap_err();
        assert!(err.contains("invalid theme version"));
    }

    /// Manifest with non-CSS path is rejected.
    #[test]
    fn test_parse_manifest_invalid_css_path() {
        let toml_str = r#"
[theme]
name = "valid-name"
version = "1.0.0"

[assets]
css = "theme.js"
"#;
        let err = ThemeManifest::parse_and_validate(toml_str).unwrap_err();
        assert!(err.contains("must end with .css"));
    }

    /// Manifest with path traversal in CSS path is rejected.
    #[test]
    fn test_parse_manifest_css_path_traversal() {
        let toml_str = r#"
[theme]
name = "valid-name"
version = "1.0.0"

[assets]
css = "../../../etc/passwd.css"
"#;
        let err = ThemeManifest::parse_and_validate(toml_str).unwrap_err();
        assert!(err.contains("no '..'"));
    }

    /// Manifest with absolute CSS path is rejected.
    #[test]
    fn test_parse_manifest_css_absolute_path() {
        let toml_str = r#"
[theme]
name = "valid-name"
version = "1.0.0"

[assets]
css = "/etc/theme.css"
"#;
        let err = ThemeManifest::parse_and_validate(toml_str).unwrap_err();
        assert!(err.contains("relative"));
    }

    /// Manifest with path traversal in assets_dir is rejected.
    #[test]
    fn test_parse_manifest_assets_dir_traversal() {
        let toml_str = r#"
[theme]
name = "valid-name"
version = "1.0.0"

[assets]
css = "theme.css"
assets_dir = "../secrets"
"#;
        let err = ThemeManifest::parse_and_validate(toml_str).unwrap_err();
        assert!(err.contains("no '..'"));
    }

    // ─── Git URL validation ──────────────────────────────────────────────

    #[test]
    fn test_validate_git_url_valid_https() {
        assert!(validate_git_url("https://github.com/org/repo.git").is_ok());
    }

    #[test]
    fn test_validate_git_url_reject_http() {
        let err = validate_git_url("http://example.com/repo").unwrap_err();
        assert!(err.contains("HTTPS"));
    }

    #[test]
    fn test_validate_git_url_reject_file() {
        let err = validate_git_url("file:///etc/passwd").unwrap_err();
        assert!(err.contains("HTTPS"));
    }

    #[test]
    fn test_validate_git_url_reject_localhost() {
        let err = validate_git_url("https://localhost/repo").unwrap_err();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_validate_git_url_reject_private_ip() {
        let err = validate_git_url("https://192.168.1.1/repo").unwrap_err();
        assert!(err.contains("private IP"));
    }

    #[test]
    fn test_validate_git_url_reject_metadata() {
        let err = validate_git_url("https://169.254.169.254/latest").unwrap_err();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn test_validate_git_url_reject_empty() {
        assert!(validate_git_url("").is_err());
    }

    // ─── Extension check ─────────────────────────────────────────────────

    #[test]
    fn test_is_allowed_extension() {
        assert!(is_allowed_extension(StdPath::new("theme.css")));
        assert!(is_allowed_extension(StdPath::new("font.woff2")));
        assert!(is_allowed_extension(StdPath::new("logo.png")));
        assert!(is_allowed_extension(StdPath::new("icon.svg")));
        assert!(!is_allowed_extension(StdPath::new("script.js")));
        assert!(!is_allowed_extension(StdPath::new("index.html")));
        assert!(!is_allowed_extension(StdPath::new("noext")));
    }

    // ─── DTO serialization ───────────────────────────────────────────────

    #[test]
    fn test_deserialize_install_theme_request() {
        let json_str = r#"{"git_url":"https://github.com/org/theme"}"#;
        let req: InstallThemeRequest = serde_json::from_str(json_str).unwrap();
        assert_eq!(req.git_url, "https://github.com/org/theme");
    }

    #[test]
    fn test_serialize_active_theme_response() {
        let resp = ActiveThemeResponse {
            id: "abc-123".into(),
            name: "dark-mode".into(),
            version: "1.0.0".into(),
            description: Some("A dark theme".into()),
            author: Some("Test".into()),
            css_url: "/api/themes/active.css".into(),
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val["name"], "dark-mode");
        assert_eq!(val["css_url"], "/api/themes/active.css");
    }

    // ─── Constants ───────────────────────────────────────────────────────

    #[test]
    fn test_default_max_theme_size() {
        assert_eq!(DEFAULT_MAX_THEME_SIZE_MB, 20);
    }

    #[test]
    fn test_allowed_extensions_contains_css() {
        assert!(ALLOWED_EXTENSIONS.contains(&"css"));
        assert!(ALLOWED_EXTENSIONS.contains(&"woff2"));
        assert!(!ALLOWED_EXTENSIONS.contains(&"js"));
        assert!(!ALLOWED_EXTENSIONS.contains(&"html"));
    }

    #[test]
    fn test_theme_name_pattern_valid() {
        let re = Regex::new(THEME_NAME_PATTERN).unwrap();
        assert!(re.is_match("dark-mode"));
        assert!(re.is_match("my-theme-v2"));
        assert!(re.is_match("ab"));
        assert!(!re.is_match("A")); // uppercase
        assert!(!re.is_match("a")); // too short (only 1 char, need 2-64)
        assert!(!re.is_match("-start")); // starts with hyphen
        assert!(!re.is_match("1start")); // starts with digit
    }
}
