//! Host functions exposed to WASM plugins.
//!
//! These are helper functions that the PluginRegistry calls on behalf of
//! plugins. Each function checks permissions before execution.

use std::collections::HashMap;

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::PluginError;
use crate::manifest::Permissions;
use soundtime_db::entities::{album, artist, plugin_config, track, user};

/// Maximum HTTP response body size (10 MB).
const MAX_HTTP_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

/// Maximum log message length from plugins.
const MAX_LOG_MESSAGE_LEN: usize = 2048;

/// Sanitize a log message from a plugin.
///
/// Strips control characters (except newline/tab), truncates to max length.
fn sanitize_log_message(message: &str) -> String {
    let cleaned: String = message
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(MAX_LOG_MESSAGE_LEN)
        .collect();
    if message.len() > MAX_LOG_MESSAGE_LEN {
        format!("{cleaned}… (truncated)")
    } else {
        cleaned
    }
}

// ─── Types shared with plugins ────────────────────────────────────────

/// Track metadata exposed to plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    pub id: String,
    pub title: String,
    pub artist_name: String,
    pub album_title: Option<String>,
    pub duration_secs: f64,
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub format: String,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub play_count: i64,
}

/// HTTP response returned by http_get / http_post.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

/// Instance information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub name: String,
    pub version: String,
    pub domain: String,
    pub track_count: u64,
    pub user_count: u64,
}

/// Paginated track list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedTracks {
    pub tracks: Vec<TrackInfo>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
    pub total_pages: u32,
}

/// User information (no sensitive data).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub role: String,
    pub created_at: String,
}

// ─── Host context ─────────────────────────────────────────────────────

/// Context for host function execution.
///
/// Holds the database connection, plugin identity, and permissions.
/// Each method checks the relevant permission before executing.
#[derive(Clone)]
pub struct HostContext {
    db: sea_orm::DatabaseConnection,
    plugin_id: Uuid,
    plugin_name: String,
    permissions: Permissions,
    http_hosts: Vec<String>,
    domain: String,
    /// Shared HTTP client for connection pooling.
    http_client: reqwest::Client,
    /// Server version string (passed from main.rs).
    server_version: String,
}

impl HostContext {
    /// Create a new host context for a plugin.
    pub fn new(
        db: sea_orm::DatabaseConnection,
        plugin_id: Uuid,
        plugin_name: String,
        permissions: Permissions,
        http_timeout_secs: u64,
        domain: String,
        server_version: String,
    ) -> Self {
        let http_hosts = permissions.http_hosts.clone();
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(http_timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        Self {
            db,
            plugin_id,
            plugin_name,
            permissions,
            http_hosts,
            domain,
            http_client,
            server_version,
        }
    }

    // ── Track read functions (no special permission) ─────────────────

    /// Get track metadata by ID.
    pub async fn get_track(&self, track_id: &str) -> Result<TrackInfo, PluginError> {
        let uuid = Uuid::parse_str(track_id)
            .map_err(|_| PluginError::HostFunction(format!("invalid track UUID: {track_id}")))?;

        let track_model = track::Entity::find_by_id(uuid)
            .one(&self.db)
            .await?
            .ok_or_else(|| PluginError::HostFunction(format!("track not found: {track_id}")))?;

        let artist_model = artist::Entity::find_by_id(track_model.artist_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| PluginError::HostFunction("artist not found".into()))?;

        let album_title = if let Some(album_id) = track_model.album_id {
            album::Entity::find_by_id(album_id)
                .one(&self.db)
                .await?
                .map(|a| a.title)
        } else {
            None
        };

        Ok(TrackInfo {
            id: track_model.id.to_string(),
            title: track_model.title,
            artist_name: artist_model.name,
            album_title,
            duration_secs: track_model.duration_secs as f64,
            genre: track_model.genre,
            year: track_model.year.map(|y| y as i32),
            format: track_model.format,
            bitrate: track_model.bitrate,
            sample_rate: track_model.sample_rate,
            play_count: track_model.play_count,
        })
    }

    /// Search tracks by query string (title match).
    pub async fn search_tracks(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<TrackInfo>, PluginError> {
        let limit = limit.min(100) as u64;

        let tracks = track::Entity::find()
            .filter(track::Column::Title.contains(query))
            .limit(limit)
            .all(&self.db)
            .await?;

        let mut results = Vec::with_capacity(tracks.len());
        for t in tracks {
            let artist_model = artist::Entity::find_by_id(t.artist_id)
                .one(&self.db)
                .await?;
            let album_title = if let Some(album_id) = t.album_id {
                album::Entity::find_by_id(album_id)
                    .one(&self.db)
                    .await?
                    .map(|a| a.title)
            } else {
                None
            };

            results.push(TrackInfo {
                id: t.id.to_string(),
                title: t.title,
                artist_name: artist_model.map(|a| a.name).unwrap_or_default(),
                album_title,
                duration_secs: t.duration_secs as f64,
                genre: t.genre,
                year: t.year.map(|y| y as i32),
                format: t.format,
                bitrate: t.bitrate,
                sample_rate: t.sample_rate,
                play_count: t.play_count,
            });
        }

        Ok(results)
    }

    /// List tracks with pagination.
    pub async fn list_tracks(
        &self,
        page: u32,
        per_page: u32,
    ) -> Result<PaginatedTracks, PluginError> {
        let per_page = per_page.clamp(1, 100);
        let page = page.max(1);

        let paginator = track::Entity::find()
            .order_by_asc(track::Column::Title)
            .paginate(&self.db, per_page as u64);

        let total = paginator.num_items().await?;
        let total_pages = paginator.num_pages().await?;
        let tracks = paginator.fetch_page((page - 1) as u64).await?;

        let mut items = Vec::with_capacity(tracks.len());
        for t in tracks {
            let artist_model = artist::Entity::find_by_id(t.artist_id)
                .one(&self.db)
                .await?;
            let album_title = if let Some(album_id) = t.album_id {
                album::Entity::find_by_id(album_id)
                    .one(&self.db)
                    .await?
                    .map(|a| a.title)
            } else {
                None
            };

            items.push(TrackInfo {
                id: t.id.to_string(),
                title: t.title,
                artist_name: artist_model.map(|a| a.name).unwrap_or_default(),
                album_title,
                duration_secs: t.duration_secs as f64,
                genre: t.genre,
                year: t.year.map(|y| y as i32),
                format: t.format,
                bitrate: t.bitrate,
                sample_rate: t.sample_rate,
                play_count: t.play_count,
            });
        }

        Ok(PaginatedTracks {
            tracks: items,
            total,
            page,
            per_page,
            total_pages: total_pages as u32,
        })
    }

    // ── Track write functions (requires write_tracks) ────────────────

    /// Set a metadata field on a track. Requires `write_tracks` permission.
    pub async fn set_track_metadata(
        &self,
        track_id: &str,
        field: &str,
        value: &str,
    ) -> Result<bool, PluginError> {
        if !self.permissions.write_tracks {
            return Err(PluginError::PermissionDenied(format!(
                "plugin '{}' does not have write_tracks permission",
                self.plugin_name
            )));
        }

        let uuid = Uuid::parse_str(track_id)
            .map_err(|_| PluginError::HostFunction(format!("invalid track UUID: {track_id}")))?;

        let track_model = track::Entity::find_by_id(uuid)
            .one(&self.db)
            .await?
            .ok_or_else(|| PluginError::HostFunction(format!("track not found: {track_id}")))?;

        let mut active: track::ActiveModel = track_model.into();

        match field {
            "title" => active.title = Set(value.to_string()),
            "genre" => active.genre = Set(Some(value.to_string())),
            _ => {
                return Err(PluginError::HostFunction(format!(
                    "unsupported metadata field: {field}"
                )));
            }
        }

        active.update(&self.db).await?;
        tracing::info!(
            plugin = %self.plugin_name,
            track_id = %track_id,
            field = %field,
            "plugin updated track metadata"
        );
        Ok(true)
    }

    /// Set lyrics on a track. Requires `write_tracks` permission.
    ///
    /// Since the tracks table does not have a dedicated lyrics column,
    /// lyrics are stored in plugin config as `lyrics:{track_id}`.
    pub async fn set_track_lyrics(
        &self,
        track_id: &str,
        lyrics: &str,
    ) -> Result<bool, PluginError> {
        if !self.permissions.write_tracks {
            return Err(PluginError::PermissionDenied(format!(
                "plugin '{}' does not have write_tracks permission",
                self.plugin_name
            )));
        }

        // Verify track exists
        let uuid = Uuid::parse_str(track_id)
            .map_err(|_| PluginError::HostFunction(format!("invalid track UUID: {track_id}")))?;

        track::Entity::find_by_id(uuid)
            .one(&self.db)
            .await?
            .ok_or_else(|| PluginError::HostFunction(format!("track not found: {track_id}")))?;

        // Store lyrics in plugin config
        let key = format!("lyrics:{track_id}");
        self.write_config_internal(&key, lyrics).await?;

        tracing::info!(
            plugin = %self.plugin_name,
            track_id = %track_id,
            "plugin set track lyrics"
        );
        Ok(true)
    }

    // ── HTTP functions (requires http_hosts) ─────────────────────────

    /// Check if a URL's host is in the allowed list (supports glob patterns).
    fn check_http_host(&self, url_str: &str) -> Result<(), PluginError> {
        if self.http_hosts.is_empty() {
            return Err(PluginError::PermissionDenied(format!(
                "plugin '{}' has no http_hosts permission",
                self.plugin_name
            )));
        }

        if self.http_hosts.iter().any(|h| h == "*") {
            return Ok(());
        }

        let url = url::Url::parse(url_str)
            .map_err(|_| PluginError::HostFunction(format!("invalid URL: {url_str}")))?;

        let host = url
            .host_str()
            .ok_or_else(|| PluginError::HostFunction(format!("URL has no host: {url_str}")))?;

        let matches = self.http_hosts.iter().any(|pattern| {
            if let Some(suffix) = pattern.strip_prefix("*.") {
                // Glob match: *.example.com matches sub.example.com, a.b.example.com
                host == suffix || host.ends_with(&format!(".{suffix}"))
            } else {
                host == pattern
            }
        });

        if !matches {
            return Err(PluginError::PermissionDenied(format!(
                "plugin '{}' is not allowed to access host '{host}'; allowed: {:?}",
                self.plugin_name, self.http_hosts
            )));
        }

        Ok(())
    }

    /// SECURITY: Block requests to private/reserved IP addresses.
    fn check_private_ip(&self, url_str: &str) -> Result<(), PluginError> {
        let url = url::Url::parse(url_str)
            .map_err(|_| PluginError::HostFunction(format!("invalid URL: {url_str}")))?;

        let host = match url.host_str() {
            Some(h) => h,
            None => return Ok(()),
        };

        // Block known private hostnames
        let blocked = [
            "localhost",
            "127.0.0.1",
            "0.0.0.0",
            "[::1]",
            "169.254.169.254",
            "metadata.google.internal",
        ];
        if blocked.contains(&host) {
            return Err(PluginError::PermissionDenied(format!(
                "HTTP requests to '{host}' are blocked (private/reserved address)"
            )));
        }

        // Block private IP ranges
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            let is_private = match ip {
                std::net::IpAddr::V4(v4) => {
                    v4.is_private() || v4.is_loopback() || v4.is_link_local()
                }
                std::net::IpAddr::V6(v6) => v6.is_loopback(),
            };
            if is_private {
                return Err(PluginError::PermissionDenied(format!(
                    "HTTP requests to private IP '{host}' are blocked"
                )));
            }
        }

        Ok(())
    }

    /// Perform an HTTP GET request. Requires `http_hosts` permission.
    pub async fn http_get(
        &self,
        url: &str,
        headers: &HashMap<String, String>,
    ) -> Result<HttpResponse, PluginError> {
        self.check_http_host(url)?;
        // SECURITY: Block private IPs even with wildcard permission
        self.check_private_ip(url)?;

        let mut req = self.http_client.get(url);
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }

        let resp = req
            .send()
            .await
            .map_err(|e| PluginError::Http(e.to_string()))?;

        // SECURITY: Limit response body size to 10 MB
        self.read_response(resp).await
    }

    /// Perform an HTTP POST request. Requires `http_hosts` permission.
    pub async fn http_post(
        &self,
        url: &str,
        body: &str,
        headers: &HashMap<String, String>,
    ) -> Result<HttpResponse, PluginError> {
        self.check_http_host(url)?;
        // SECURITY: Block private IPs even with wildcard permission
        self.check_private_ip(url)?;

        let mut req = self.http_client.post(url).body(body.to_string());
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }

        let resp = req
            .send()
            .await
            .map_err(|e| PluginError::Http(e.to_string()))?;

        // SECURITY: Limit response body size to 10 MB
        self.read_response(resp).await
    }

    /// Read HTTP response with body size limit.
    async fn read_response(&self, resp: reqwest::Response) -> Result<HttpResponse, PluginError> {
        let status = resp.status().as_u16();
        let resp_headers: HashMap<String, String> = resp
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let body_bytes = resp
            .bytes()
            .await
            .map_err(|e| PluginError::Http(e.to_string()))?;

        if body_bytes.len() > MAX_HTTP_RESPONSE_BYTES {
            return Err(PluginError::Http(format!(
                "response body too large: {} bytes (max: {} bytes)",
                body_bytes.len(),
                MAX_HTTP_RESPONSE_BYTES
            )));
        }

        let body = String::from_utf8_lossy(&body_bytes).to_string();

        Ok(HttpResponse {
            status,
            headers: resp_headers,
            body,
        })
    }

    // ── Config functions (requires config_access) ────────────────────

    /// Get a config value for this plugin. Requires `config_access` permission.
    pub async fn get_config(&self, key: &str) -> Result<Option<String>, PluginError> {
        if !self.permissions.config_access {
            return Err(PluginError::PermissionDenied(format!(
                "plugin '{}' does not have config_access permission",
                self.plugin_name
            )));
        }

        let config = plugin_config::Entity::find()
            .filter(plugin_config::Column::PluginId.eq(self.plugin_id))
            .filter(plugin_config::Column::Key.eq(key))
            .one(&self.db)
            .await?;

        Ok(config.map(|c| c.value))
    }

    /// Set a config value for this plugin. Requires `config_access` permission.
    pub async fn set_config(&self, key: &str, value: &str) -> Result<bool, PluginError> {
        if !self.permissions.config_access {
            return Err(PluginError::PermissionDenied(format!(
                "plugin '{}' does not have config_access permission",
                self.plugin_name
            )));
        }

        self.write_config_internal(key, value).await
    }

    /// Internal config write — bypasses permission check.
    async fn write_config_internal(&self, key: &str, value: &str) -> Result<bool, PluginError> {
        let existing = plugin_config::Entity::find()
            .filter(plugin_config::Column::PluginId.eq(self.plugin_id))
            .filter(plugin_config::Column::Key.eq(key))
            .one(&self.db)
            .await?;

        let now = Utc::now().fixed_offset();

        if let Some(existing) = existing {
            let mut active: plugin_config::ActiveModel = existing.into();
            active.value = Set(value.to_string());
            active.updated_at = Set(now);
            active.update(&self.db).await?;
        } else {
            let new_config = plugin_config::ActiveModel {
                id: Set(Uuid::new_v4()),
                plugin_id: Set(self.plugin_id),
                key: Set(key.to_string()),
                value: Set(value.to_string()),
                created_at: Set(now),
                updated_at: Set(now),
            };
            new_config.insert(&self.db).await?;
        }

        Ok(true)
    }

    // ── Logging functions (no permission) ────────────────────────────

    /// Log an info message on behalf of the plugin.
    pub fn log_info(&self, message: &str) {
        let msg = sanitize_log_message(message);
        tracing::info!(plugin = %self.plugin_name, "{msg}");
    }

    /// Log a warning message on behalf of the plugin.
    pub fn log_warn(&self, message: &str) {
        let msg = sanitize_log_message(message);
        tracing::warn!(plugin = %self.plugin_name, "{msg}");
    }

    /// Log an error message on behalf of the plugin.
    pub fn log_error(&self, message: &str) {
        let msg = sanitize_log_message(message);
        tracing::error!(plugin = %self.plugin_name, "{msg}");
    }

    // ── Instance info (no permission) ────────────────────────────────

    /// Get instance information.
    pub async fn get_instance_info(&self) -> Result<InstanceInfo, PluginError> {
        let track_count = track::Entity::find().count(&self.db).await?;
        let user_count = user::Entity::find().count(&self.db).await?;

        Ok(InstanceInfo {
            name: "SoundTime".to_string(),
            version: self.server_version.clone(),
            domain: self.domain.clone(),
            track_count,
            user_count,
        })
    }

    // ── User info (requires read_users) ──────────────────────────────

    /// Get user information. Requires `read_users` permission.
    ///
    /// Never exposes password hashes or JWT tokens.
    pub async fn get_user_info(&self, user_id: &str) -> Result<UserInfo, PluginError> {
        if !self.permissions.read_users {
            return Err(PluginError::PermissionDenied(format!(
                "plugin '{}' does not have read_users permission",
                self.plugin_name
            )));
        }

        let uuid = Uuid::parse_str(user_id)
            .map_err(|_| PluginError::HostFunction(format!("invalid user UUID: {user_id}")))?;

        let user_model = user::Entity::find_by_id(uuid)
            .one(&self.db)
            .await?
            .ok_or_else(|| PluginError::HostFunction(format!("user not found: {user_id}")))?;

        Ok(UserInfo {
            id: user_model.id.to_string(),
            username: user_model.username,
            display_name: user_model.display_name,
            role: user_model.role.as_str().to_string(),
            created_at: user_model.created_at.to_rfc3339(),
        })
    }

    // ── Event emission (no special permission) ───────────────────────

    /// Emit a custom event. Returns true if the event name is valid.
    ///
    /// The actual dispatch is handled by the registry, not here.
    pub fn emit_event(&self, event_name: &str, _payload: &str) -> Result<bool, PluginError> {
        if event_name.is_empty() {
            return Err(PluginError::HostFunction(
                "event name cannot be empty".into(),
            ));
        }
        tracing::info!(
            plugin = %self.plugin_name,
            event = %event_name,
            "plugin emitted custom event"
        );
        Ok(true)
    }

    /// Get the current UTC timestamp in ISO 8601 format.
    pub fn get_current_timestamp(&self) -> String {
        Utc::now().to_rfc3339()
    }

    // ── Accessors ────────────────────────────────────────────────────

    /// Returns the plugin ID.
    pub fn plugin_id(&self) -> Uuid {
        self.plugin_id
    }

    /// Returns the plugin name.
    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }

    /// Returns the permissions.
    pub fn permissions(&self) -> &Permissions {
        &self.permissions
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_info_serialization() {
        let info = TrackInfo {
            id: "abc".into(),
            title: "Test".into(),
            artist_name: "Artist".into(),
            album_title: Some("Album".into()),
            duration_secs: 180.5,
            genre: Some("Rock".into()),
            year: Some(2024),
            format: "flac".into(),
            bitrate: Some(320),
            sample_rate: Some(44100),
            play_count: 42,
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: TrackInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, "abc");
        assert_eq!(deserialized.title, "Test");
        assert_eq!(deserialized.play_count, 42);
    }

    #[test]
    fn test_http_response_serialization() {
        let resp = HttpResponse {
            status: 200,
            headers: HashMap::from([("content-type".into(), "application/json".into())]),
            body: "{\"ok\": true}".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let deserialized: HttpResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.status, 200);
        assert_eq!(deserialized.body, "{\"ok\": true}");
    }

    #[test]
    fn test_instance_info_serialization() {
        let info = InstanceInfo {
            name: "SoundTime".into(),
            version: "0.5.0".into(),
            domain: "example.com".into(),
            track_count: 1000,
            user_count: 50,
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: InstanceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.track_count, 1000);
    }

    #[test]
    fn test_paginated_tracks_serialization() {
        let paginated = PaginatedTracks {
            tracks: vec![],
            total: 0,
            page: 1,
            per_page: 20,
            total_pages: 0,
        };
        let json = serde_json::to_string(&paginated).unwrap();
        let deserialized: PaginatedTracks = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.page, 1);
        assert!(deserialized.tracks.is_empty());
    }

    #[test]
    fn test_user_info_serialization() {
        let info = UserInfo {
            id: "user-123".into(),
            username: "testuser".into(),
            display_name: Some("Test User".into()),
            role: "admin".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: UserInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.username, "testuser");
        assert_eq!(deserialized.role, "admin");
    }

    #[test]
    fn test_sanitize_log_message() {
        assert_eq!(sanitize_log_message("hello world"), "hello world");
        assert_eq!(sanitize_log_message("line1\nline2"), "line1\nline2");
        assert_eq!(sanitize_log_message("tab\there"), "tab\there");
        // Control chars stripped
        assert_eq!(sanitize_log_message("bad\x00\x01\x02chars"), "badchars");
        // Long message truncated
        let long = "x".repeat(3000);
        let result = sanitize_log_message(&long);
        assert!(result.len() < 3000);
        assert!(result.ends_with("… (truncated)"));
    }

    #[test]
    fn test_http_host_glob_matching() {
        // Test the glob matching logic directly
        let pattern = "*.cdn.example.com";
        let suffix = pattern.strip_prefix("*.").unwrap();

        assert!(
            "img.cdn.example.com" == suffix
                || "img.cdn.example.com".ends_with(&format!(".{suffix}"))
        );
        assert!("a.b.cdn.example.com".ends_with(&format!(".{suffix}")));
        assert!(!"evil.com".ends_with(&format!(".{suffix}")));
    }

    // ─── Helper constructors ────────────────────────────────────────

    /// Create a test HostContext with the given http_hosts permission.
    fn test_context(http_hosts: Vec<String>) -> HostContext {
        let db = sea_orm::DatabaseConnection::Disconnected;
        let perms = Permissions {
            http_hosts,
            write_tracks: false,
            config_access: false,
            read_users: false,
            events: vec![],
        };
        HostContext::new(
            db,
            Uuid::new_v4(),
            "test-plugin".to_string(),
            perms,
            10,
            "localhost".to_string(),
            "0.1.0".to_string(),
        )
    }

    /// Create a test HostContext with fine-grained permissions.
    fn test_context_with_perms(
        http_hosts: Vec<String>,
        write_tracks: bool,
        config_access: bool,
        read_users: bool,
    ) -> HostContext {
        let db = sea_orm::DatabaseConnection::Disconnected;
        let perms = Permissions {
            http_hosts,
            write_tracks,
            config_access,
            read_users,
            events: vec![],
        };
        HostContext::new(
            db,
            Uuid::new_v4(),
            "test-plugin".to_string(),
            perms,
            10,
            "localhost".to_string(),
            "0.1.0".to_string(),
        )
    }

    // ─── check_http_host tests ──────────────────────────────────────

    #[test]
    fn test_check_http_host_no_permissions() {
        let ctx = test_context(vec![]);
        let err = ctx.check_http_host("https://example.com/path").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
        assert!(err.to_string().contains("no http_hosts permission"));
    }

    #[test]
    fn test_check_http_host_wildcard_allows_all() {
        let ctx = test_context(vec!["*".into()]);
        ctx.check_http_host("https://anything.example.com/path")
            .unwrap();
        ctx.check_http_host("https://evil.com/hack").unwrap();
        ctx.check_http_host("https://192.168.1.1/admin").unwrap();
    }

    #[test]
    fn test_check_http_host_exact_match() {
        let ctx = test_context(vec!["api.example.com".into()]);
        ctx.check_http_host("https://api.example.com/path").unwrap();
        ctx.check_http_host("https://api.example.com/other?q=1")
            .unwrap();
        ctx.check_http_host("http://api.example.com/").unwrap();
    }

    #[test]
    fn test_check_http_host_exact_match_reject() {
        let ctx = test_context(vec!["api.example.com".into()]);
        let err = ctx.check_http_host("https://evil.com/path").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
        assert!(err.to_string().contains("not allowed to access host"));
    }

    #[test]
    fn test_check_http_host_glob_match() {
        let ctx = test_context(vec!["*.example.com".into()]);
        ctx.check_http_host("https://sub.example.com/path").unwrap();
        ctx.check_http_host("https://api.example.com/v1").unwrap();
    }

    #[test]
    fn test_check_http_host_glob_nested() {
        let ctx = test_context(vec!["*.example.com".into()]);
        ctx.check_http_host("https://a.b.example.com/path").unwrap();
        ctx.check_http_host("https://deep.nested.sub.example.com/")
            .unwrap();
    }

    #[test]
    fn test_check_http_host_glob_no_match() {
        let ctx = test_context(vec!["*.example.com".into()]);
        let err = ctx.check_http_host("https://evil.com/path").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    #[test]
    fn test_check_http_host_glob_base_domain() {
        // *.example.com should also match example.com itself (suffix == host)
        let ctx = test_context(vec!["*.example.com".into()]);
        ctx.check_http_host("https://example.com/path").unwrap();
    }

    #[test]
    fn test_check_http_host_invalid_url() {
        // Use a non-wildcard host so URL parsing is actually reached
        let ctx = test_context(vec!["example.com".into()]);
        let err = ctx.check_http_host("not a url at all").unwrap_err();
        assert!(matches!(err, PluginError::HostFunction(_)));
        assert!(err.to_string().contains("invalid URL"));
    }

    #[test]
    fn test_check_http_host_url_no_host() {
        // Use a non-wildcard host so URL parsing is actually reached
        let ctx = test_context(vec!["example.com".into()]);
        let err = ctx
            .check_http_host("data:text/html,<h1>hi</h1>")
            .unwrap_err();
        assert!(matches!(err, PluginError::HostFunction(_)));
        assert!(err.to_string().contains("no host"));
    }

    #[test]
    fn test_check_http_host_multiple_patterns() {
        let ctx = test_context(vec!["api.example.com".into(), "*.cdn.example.com".into()]);
        ctx.check_http_host("https://api.example.com/v1").unwrap();
        ctx.check_http_host("https://img.cdn.example.com/pic.jpg")
            .unwrap();
        let err = ctx.check_http_host("https://evil.com/").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    // ─── check_private_ip tests ─────────────────────────────────────

    #[test]
    fn test_check_private_ip_localhost() {
        let ctx = test_context(vec!["*".into()]);
        let err = ctx.check_private_ip("https://localhost/path").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
        assert!(err.to_string().contains("blocked"));
    }

    #[test]
    fn test_check_private_ip_127_0_0_1() {
        let ctx = test_context(vec!["*".into()]);
        let err = ctx.check_private_ip("https://127.0.0.1/path").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    #[test]
    fn test_check_private_ip_0_0_0_0() {
        let ctx = test_context(vec!["*".into()]);
        let err = ctx.check_private_ip("https://0.0.0.0/path").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    #[test]
    fn test_check_private_ip_ipv6_loopback() {
        let ctx = test_context(vec!["*".into()]);
        let err = ctx.check_private_ip("https://[::1]/path").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    #[test]
    fn test_check_private_ip_aws_metadata() {
        let ctx = test_context(vec!["*".into()]);
        let err = ctx
            .check_private_ip("https://169.254.169.254/latest/meta-data/")
            .unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    #[test]
    fn test_check_private_ip_gcp_metadata() {
        let ctx = test_context(vec!["*".into()]);
        let err = ctx
            .check_private_ip("https://metadata.google.internal/computeMetadata/v1/")
            .unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    #[test]
    fn test_check_private_ip_10_range() {
        let ctx = test_context(vec!["*".into()]);
        let err = ctx.check_private_ip("https://10.0.0.1/path").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
        assert!(err.to_string().contains("private IP"));
    }

    #[test]
    fn test_check_private_ip_172_16_range() {
        let ctx = test_context(vec!["*".into()]);
        let err = ctx.check_private_ip("https://172.16.0.1/path").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    #[test]
    fn test_check_private_ip_192_168_range() {
        let ctx = test_context(vec!["*".into()]);
        let err = ctx
            .check_private_ip("https://192.168.1.1/path")
            .unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    #[test]
    fn test_check_private_ip_public_ip_ok() {
        let ctx = test_context(vec!["*".into()]);
        ctx.check_private_ip("https://8.8.8.8/path").unwrap();
    }

    #[test]
    fn test_check_private_ip_public_domain_ok() {
        let ctx = test_context(vec!["*".into()]);
        ctx.check_private_ip("https://api.example.com/path")
            .unwrap();
    }

    #[test]
    fn test_check_private_ip_link_local() {
        let ctx = test_context(vec!["*".into()]);
        let err = ctx
            .check_private_ip("https://169.254.0.1/path")
            .unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    #[test]
    fn test_check_private_ip_127_other_loopback() {
        // 127.x.x.x range is all loopback
        let ctx = test_context(vec!["*".into()]);
        let err = ctx.check_private_ip("https://127.0.0.2/path").unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
    }

    // ─── Permission check tests (async) ─────────────────────────────

    #[tokio::test]
    async fn test_set_track_metadata_no_permission() {
        let ctx = test_context_with_perms(vec![], false, false, false);
        let err = ctx
            .set_track_metadata("00000000-0000-0000-0000-000000000000", "title", "New Title")
            .await
            .unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
        assert!(err.to_string().contains("write_tracks"));
    }

    #[tokio::test]
    async fn test_set_track_lyrics_no_permission() {
        let ctx = test_context_with_perms(vec![], false, false, false);
        let err = ctx
            .set_track_lyrics("00000000-0000-0000-0000-000000000000", "Some lyrics")
            .await
            .unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
        assert!(err.to_string().contains("write_tracks"));
    }

    #[tokio::test]
    async fn test_get_config_no_permission() {
        let ctx = test_context_with_perms(vec![], false, false, false);
        let err = ctx.get_config("some_key").await.unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
        assert!(err.to_string().contains("config_access"));
    }

    #[tokio::test]
    async fn test_set_config_no_permission() {
        let ctx = test_context_with_perms(vec![], false, false, false);
        let err = ctx.set_config("key", "value").await.unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
        assert!(err.to_string().contains("config_access"));
    }

    #[tokio::test]
    async fn test_get_user_info_no_permission() {
        let ctx = test_context_with_perms(vec![], false, false, false);
        let err = ctx
            .get_user_info("00000000-0000-0000-0000-000000000000")
            .await
            .unwrap_err();
        assert!(matches!(err, PluginError::PermissionDenied(_)));
        assert!(err.to_string().contains("read_users"));
    }

    // ── Permission granted → hits disconnected DB (proves check passed)

    #[tokio::test]
    #[should_panic(expected = "Disconnected")]
    async fn test_set_track_metadata_with_permission_hits_db() {
        let ctx = test_context_with_perms(vec![], true, false, false);
        // Permission check passes, then DB access panics with "Disconnected"
        let _ = ctx
            .set_track_metadata("00000000-0000-0000-0000-000000000000", "title", "New Title")
            .await;
    }

    #[tokio::test]
    #[should_panic(expected = "Disconnected")]
    async fn test_get_config_with_permission_hits_db() {
        let ctx = test_context_with_perms(vec![], false, true, false);
        let _ = ctx.get_config("some_key").await;
    }

    #[tokio::test]
    #[should_panic(expected = "Disconnected")]
    async fn test_get_user_info_with_permission_hits_db() {
        let ctx = test_context_with_perms(vec![], false, false, true);
        let _ = ctx
            .get_user_info("00000000-0000-0000-0000-000000000000")
            .await;
    }

    // ─── emit_event tests ───────────────────────────────────────────

    #[test]
    fn test_emit_event_empty_name() {
        let ctx = test_context(vec![]);
        let err = ctx.emit_event("", "{}").unwrap_err();
        assert!(matches!(err, PluginError::HostFunction(_)));
        assert!(err.to_string().contains("event name cannot be empty"));
    }

    #[test]
    fn test_emit_event_valid() {
        let ctx = test_context(vec![]);
        let result = ctx.emit_event("my_event", "{\"key\": \"value\"}").unwrap();
        assert!(result);
    }

    #[test]
    fn test_emit_event_with_payload() {
        let ctx = test_context(vec![]);
        let result = ctx.emit_event("track_processed", "").unwrap();
        assert!(result);
    }

    // ─── get_current_timestamp tests ────────────────────────────────

    #[test]
    fn test_get_current_timestamp_format() {
        let ctx = test_context(vec![]);
        let ts = ctx.get_current_timestamp();
        // Should be parseable as RFC 3339
        let parsed = chrono::DateTime::parse_from_rfc3339(&ts);
        assert!(parsed.is_ok(), "timestamp not valid RFC 3339: {ts}");
    }

    #[test]
    fn test_get_current_timestamp_is_recent() {
        let ctx = test_context(vec![]);
        let ts = ctx.get_current_timestamp();
        let parsed = chrono::DateTime::parse_from_rfc3339(&ts).unwrap();
        let now = Utc::now();
        let diff = now.signed_duration_since(parsed);
        // Should be within 2 seconds of now
        assert!(
            diff.num_seconds().abs() < 2,
            "timestamp too far from now: {ts}"
        );
    }

    // ─── Extended sanitize_log_message tests ────────────────────────

    #[test]
    fn test_sanitize_log_message_empty() {
        assert_eq!(sanitize_log_message(""), "");
    }

    #[test]
    fn test_sanitize_log_message_carriage_return_stripped() {
        // \r is a control char that is NOT \n or \t, so it should be stripped
        assert_eq!(sanitize_log_message("hello\r\nworld"), "hello\nworld");
    }

    #[test]
    fn test_sanitize_log_message_bell_stripped() {
        assert_eq!(sanitize_log_message("alert\x07!"), "alert!");
    }

    #[test]
    fn test_sanitize_log_message_escape_stripped() {
        // ESC character (0x1B) used in ANSI codes
        assert_eq!(sanitize_log_message("\x1b[31mred\x1b[0m"), "[31mred[0m");
    }

    #[test]
    fn test_sanitize_log_message_exact_max_length() {
        let msg = "x".repeat(MAX_LOG_MESSAGE_LEN);
        let result = sanitize_log_message(&msg);
        // Exactly at max length should NOT be truncated
        assert_eq!(result.len(), MAX_LOG_MESSAGE_LEN);
        assert!(!result.ends_with("… (truncated)"));
    }

    #[test]
    fn test_sanitize_log_message_one_over_max() {
        let msg = "x".repeat(MAX_LOG_MESSAGE_LEN + 1);
        let result = sanitize_log_message(&msg);
        assert!(result.ends_with("… (truncated)"));
    }

    // ─── Accessor tests ─────────────────────────────────────────────

    #[test]
    fn test_host_context_plugin_name() {
        let ctx = test_context(vec![]);
        assert_eq!(ctx.plugin_name(), "test-plugin");
    }

    #[test]
    fn test_host_context_permissions() {
        let ctx = test_context_with_perms(vec!["example.com".into()], true, false, true);
        let perms = ctx.permissions();
        assert!(perms.write_tracks);
        assert!(!perms.config_access);
        assert!(perms.read_users);
        assert_eq!(perms.http_hosts, vec!["example.com"]);
    }
}
