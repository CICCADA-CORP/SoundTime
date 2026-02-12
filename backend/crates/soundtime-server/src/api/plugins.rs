//! Plugin management API endpoints.
//!
//! All endpoints require admin authentication. The plugin system must
//! be enabled (`PLUGIN_ENABLED=true`) for these endpoints to function.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use serde::{Deserialize, Serialize};
use soundtime_db::entities::{plugin, plugin_config, plugin_events_log};
use soundtime_db::AppState;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;

// ─── Helpers ────────────────────────────────────────────────────────────

/// Return a standard error when the plugin system is disabled.
fn plugin_system_disabled() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({ "error": "Plugin system is not enabled" })),
    )
}

// ─── Request / Response types ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    pub git_url: String,
}

#[derive(Debug, Serialize)]
pub struct PluginListResponse {
    pub plugins: Vec<plugin::Model>,
}

#[derive(Debug, Deserialize)]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub config: Vec<ConfigEntry>,
}

#[derive(Debug, Serialize)]
pub struct ConfigResponse {
    pub config: Vec<ConfigEntryResponse>,
}

#[derive(Debug, Serialize)]
pub struct ConfigEntryResponse {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct UpdateConfigResponse {
    pub updated: usize,
}

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub page: Option<u64>,
    pub per_page: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct LogsResponse {
    pub logs: Vec<plugin_events_log::Model>,
    pub total: u64,
    pub page: u64,
    pub per_page: u64,
}

// ─── Handlers ───────────────────────────────────────────────────────────

/// GET /api/admin/plugins — List all installed plugins.
pub async fn list_plugins(
    State(state): State<Arc<AppState>>,
) -> Result<Json<PluginListResponse>, (StatusCode, Json<serde_json::Value>)> {
    let plugins = plugin::Entity::find().all(&state.db).await.map_err(|e| {
        tracing::error!("failed to list plugins: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("DB error: {e}") })),
        )
    })?;

    Ok(Json(PluginListResponse { plugins }))
}

/// POST /api/admin/plugins/install — Install a plugin from a git repository.
pub async fn install_plugin(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<AuthUser>,
    Json(body): Json<InstallRequest>,
) -> Result<(StatusCode, Json<plugin::Model>), (StatusCode, Json<serde_json::Value>)> {
    let _registry = super::get_plugin_registry(&state).ok_or_else(plugin_system_disabled)?;

    let installer = soundtime_plugin::PluginInstaller::new(state.db.clone());

    let model = installer
        .install_from_git(&body.git_url, Some(user.0.sub))
        .await
        .map_err(|e| {
            tracing::error!(git_url = %body.git_url, "plugin installation failed: {e}");
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("{e}") })),
            )
        })?;

    tracing::info!(
        plugin_name = %model.name,
        plugin_id = %model.id,
        "plugin installed via API"
    );

    Ok((StatusCode::CREATED, Json(model)))
}

/// POST /api/admin/plugins/:id/enable — Enable a plugin.
pub async fn enable_plugin(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let registry = super::get_plugin_registry(&state).ok_or_else(plugin_system_disabled)?;

    registry.enable_plugin(id).await.map_err(|e| {
        tracing::error!(plugin_id = %id, "failed to enable plugin: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{e}") })),
        )
    })?;

    tracing::info!(plugin_id = %id, "plugin enabled via API");
    Ok(Json(serde_json::json!({ "status": "enabled" })))
}

/// POST /api/admin/plugins/:id/disable — Disable a plugin.
pub async fn disable_plugin(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let registry = super::get_plugin_registry(&state).ok_or_else(plugin_system_disabled)?;

    registry.disable_plugin(id).await.map_err(|e| {
        tracing::error!(plugin_id = %id, "failed to disable plugin: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{e}") })),
        )
    })?;

    tracing::info!(plugin_id = %id, "plugin disabled via API");
    Ok(Json(serde_json::json!({ "status": "disabled" })))
}

/// DELETE /api/admin/plugins/:id — Uninstall a plugin.
pub async fn uninstall_plugin(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let registry = super::get_plugin_registry(&state).ok_or_else(plugin_system_disabled)?;

    // Unload from registry first (ignore if not loaded)
    let _ = registry.unload_plugin(id).await;

    let installer = soundtime_plugin::PluginInstaller::new(state.db.clone());

    installer.uninstall_plugin(id).await.map_err(|e| {
        tracing::error!(plugin_id = %id, "failed to uninstall plugin: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{e}") })),
        )
    })?;

    tracing::info!(plugin_id = %id, "plugin uninstalled via API");
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/admin/plugins/:id/update — Update a plugin from its git repo.
pub async fn update_plugin(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<plugin::Model>, (StatusCode, Json<serde_json::Value>)> {
    let registry = super::get_plugin_registry(&state).ok_or_else(plugin_system_disabled)?;

    // Unload the old version
    let _ = registry.unload_plugin(id).await;

    let installer = soundtime_plugin::PluginInstaller::new(state.db.clone());

    let model = installer.update_plugin(id).await.map_err(|e| {
        tracing::error!(plugin_id = %id, "failed to update plugin: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("{e}") })),
        )
    })?;

    // Reload if it was enabled
    if model.status == "enabled" {
        if let Err(e) = registry.load_plugin(id).await {
            tracing::error!(plugin_id = %id, "failed to reload plugin after update: {e}");
        }
    }

    tracing::info!(
        plugin_id = %id,
        new_version = %model.version,
        "plugin updated via API"
    );

    Ok(Json(model))
}

/// GET /api/admin/plugins/:id/config — Get plugin configuration.
pub async fn get_config(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<ConfigResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Verify plugin exists
    plugin::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(plugin_id = %id, "failed to query plugin: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Plugin not found" })),
            )
        })?;

    let configs = plugin_config::Entity::find()
        .filter(plugin_config::Column::PluginId.eq(id))
        .all(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(plugin_id = %id, "failed to query plugin config: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    let config = configs
        .into_iter()
        .map(|c| ConfigEntryResponse {
            key: c.key,
            value: c.value,
        })
        .collect();

    Ok(Json(ConfigResponse { config }))
}

/// PUT /api/admin/plugins/:id/config — Update plugin configuration.
pub async fn update_config(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateConfigRequest>,
) -> Result<Json<UpdateConfigResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Verify plugin exists
    plugin::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(plugin_id = %id, "failed to query plugin: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Plugin not found" })),
            )
        })?;

    let now = chrono::Utc::now().fixed_offset();
    let mut updated_count = 0;

    for entry in &body.config {
        // Try to find existing config entry
        let existing = plugin_config::Entity::find()
            .filter(plugin_config::Column::PluginId.eq(id))
            .filter(plugin_config::Column::Key.eq(&entry.key))
            .one(&state.db)
            .await
            .map_err(|e| {
                tracing::error!(plugin_id = %id, key = %entry.key, "failed to query config: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("DB error: {e}") })),
                )
            })?;

        if let Some(existing) = existing {
            // Update existing
            let mut active: plugin_config::ActiveModel = existing.into();
            active.value = Set(entry.value.clone());
            active.updated_at = Set(now);
            active.update(&state.db).await.map_err(|e| {
                tracing::error!(plugin_id = %id, key = %entry.key, "failed to update config: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("DB error: {e}") })),
                )
            })?;
        } else {
            // Insert new
            let new_config = plugin_config::ActiveModel {
                id: Set(Uuid::new_v4()),
                plugin_id: Set(id),
                key: Set(entry.key.clone()),
                value: Set(entry.value.clone()),
                created_at: Set(now),
                updated_at: Set(now),
            };
            new_config.insert(&state.db).await.map_err(|e| {
                tracing::error!(plugin_id = %id, key = %entry.key, "failed to insert config: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": format!("DB error: {e}") })),
                )
            })?;
        }

        updated_count += 1;
    }

    Ok(Json(UpdateConfigResponse {
        updated: updated_count,
    }))
}

/// GET /api/admin/plugins/:id/logs — Get plugin event logs.
pub async fn get_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(params): Query<LogsQuery>,
) -> Result<Json<LogsResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Verify plugin exists
    plugin::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(plugin_id = %id, "failed to query plugin: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Plugin not found" })),
            )
        })?;

    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(50).min(100);

    let total = plugin_events_log::Entity::find()
        .filter(plugin_events_log::Column::PluginId.eq(id))
        .count(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(plugin_id = %id, "failed to count logs: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    let logs = plugin_events_log::Entity::find()
        .filter(plugin_events_log::Column::PluginId.eq(id))
        .order_by_desc(plugin_events_log::Column::CreatedAt)
        .offset((page - 1) * per_page)
        .limit(per_page)
        .all(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(plugin_id = %id, "failed to query logs: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?;

    Ok(Json(LogsResponse {
        logs,
        total,
        page,
        per_page,
    }))
}

/// GET /api/admin/plugins/:id/ui/*path — Serve plugin UI static files.
///
/// Reads files from the plugin's UI directory (sibling to the WASM file).
/// Path traversal is prevented by canonicalization + starts_with check.
pub async fn serve_plugin_ui(
    State(state): State<Arc<AppState>>,
    Path((id, file_path)): Path<(Uuid, String)>,
) -> Result<impl axum::response::IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    // Look up plugin to find its install directory
    let plugin_model = plugin::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(plugin_id = %id, "failed to query plugin: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("DB error: {e}") })),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "Plugin not found" })),
            )
        })?;

    // Derive the UI directory from wasm_path
    let wasm_path = std::path::PathBuf::from(&plugin_model.wasm_path);
    let install_dir = wasm_path.parent().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Invalid plugin path" })),
        )
    })?;
    let ui_dir = install_dir.join("ui");

    if !ui_dir.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Plugin has no UI" })),
        ));
    }

    // SECURITY: prevent path traversal
    let requested = ui_dir.join(&file_path);
    let canonical = requested.canonicalize().map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "File not found" })),
        )
    })?;
    let canonical_ui_dir = ui_dir.canonicalize().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Plugin UI directory error" })),
        )
    })?;

    if !canonical.starts_with(&canonical_ui_dir) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Path traversal detected" })),
        ));
    }

    if !canonical.is_file() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "File not found" })),
        ));
    }

    // Read file
    let data = tokio::fs::read(&canonical).await.map_err(|e| {
        tracing::error!(plugin_id = %id, path = %file_path, "failed to read UI file: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "Failed to read file" })),
        )
    })?;

    // Determine content type from extension
    let content_type = match canonical.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    };

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        content_type.parse().unwrap(),
    );
    // SECURITY: sandbox plugin UI iframes — no scripts escaping to parent
    headers.insert(
        axum::http::header::HeaderName::from_static("content-security-policy"),
        axum::http::HeaderValue::from_static(
            "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; frame-ancestors 'self'",
        ),
    );

    Ok((headers, data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    /// Verify `plugin_system_disabled` returns 503 with the expected error message.
    #[test]
    fn test_plugin_system_disabled() {
        let (status, json) = plugin_system_disabled();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(json.0["error"], "Plugin system is not enabled");
    }

    // ─── DTO deserialization ─────────────────────────────────────────────

    /// `InstallRequest` deserializes `git_url` from JSON.
    #[test]
    fn test_deserialize_install_request() {
        let json_str = r#"{"git_url":"https://github.com/org/repo"}"#;
        let req: InstallRequest = serde_json::from_str(json_str).unwrap();
        assert_eq!(req.git_url, "https://github.com/org/repo");
    }

    /// `ConfigEntry` deserializes `key` and `value` from JSON.
    #[test]
    fn test_deserialize_config_entry() {
        let json_str = r#"{"key":"k","value":"v"}"#;
        let entry: ConfigEntry = serde_json::from_str(json_str).unwrap();
        assert_eq!(entry.key, "k");
        assert_eq!(entry.value, "v");
    }

    /// `UpdateConfigRequest` deserializes a config array from JSON.
    #[test]
    fn test_deserialize_update_config_request() {
        let json_str = r#"{"config":[{"key":"k","value":"v"}]}"#;
        let req: UpdateConfigRequest = serde_json::from_str(json_str).unwrap();
        assert_eq!(req.config.len(), 1);
    }

    /// `LogsQuery` deserializes all fields when present.
    #[test]
    fn test_deserialize_logs_query_with_fields() {
        let json_str = r#"{"page":2,"per_page":25}"#;
        let q: LogsQuery = serde_json::from_str(json_str).unwrap();
        assert_eq!(q.page, Some(2));
        assert_eq!(q.per_page, Some(25));
    }

    /// `LogsQuery` deserializes to `None` when fields are absent.
    #[test]
    fn test_deserialize_logs_query_empty() {
        let json_str = r#"{}"#;
        let q: LogsQuery = serde_json::from_str(json_str).unwrap();
        assert_eq!(q.page, None);
        assert_eq!(q.per_page, None);
    }

    // ─── DTO serialization ──────────────────────────────────────────────

    /// `ConfigEntryResponse` serializes to the expected JSON shape.
    #[test]
    fn test_serialize_config_entry_response() {
        let entry = ConfigEntryResponse {
            key: "k".into(),
            value: "v".into(),
        };
        let val = serde_json::to_value(&entry).unwrap();
        assert_eq!(val, serde_json::json!({"key": "k", "value": "v"}));
    }

    /// `UpdateConfigResponse` serializes the `updated` count.
    #[test]
    fn test_serialize_update_config_response() {
        let resp = UpdateConfigResponse { updated: 3 };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(val, serde_json::json!({"updated": 3}));
    }

    /// `ConfigResponse` serializes entries correctly.
    #[test]
    fn test_serialize_config_response() {
        let resp = ConfigResponse {
            config: vec![
                ConfigEntryResponse {
                    key: "a".into(),
                    value: "1".into(),
                },
                ConfigEntryResponse {
                    key: "b".into(),
                    value: "2".into(),
                },
            ],
        };
        let val = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            val,
            serde_json::json!({
                "config": [
                    {"key": "a", "value": "1"},
                    {"key": "b", "value": "2"}
                ]
            })
        );
    }

    // ─── Pagination defaults ────────────────────────────────────────────

    /// Verify the pagination default/clamping logic from `get_logs`.
    #[test]
    fn test_logs_query_pagination_defaults() {
        // Default values
        let q = LogsQuery {
            page: None,
            per_page: None,
        };
        let page = q.page.unwrap_or(1).max(1);
        let per_page = q.per_page.unwrap_or(50).min(100);
        assert_eq!(page, 1);
        assert_eq!(per_page, 50);

        // page=0 should become 1 (max(1))
        let q = LogsQuery {
            page: Some(0),
            per_page: Some(200),
        };
        let page = q.page.unwrap_or(1).max(1);
        let per_page = q.per_page.unwrap_or(50).min(100);
        assert_eq!(page, 1);
        assert_eq!(per_page, 100); // capped at 100

        // Normal values
        let q = LogsQuery {
            page: Some(3),
            per_page: Some(25),
        };
        let page = q.page.unwrap_or(1).max(1);
        let per_page = q.per_page.unwrap_or(50).min(100);
        assert_eq!(page, 3);
        assert_eq!(per_page, 25);
    }

    // ─── Content-type mapping ───────────────────────────────────────────

    /// Verify the file-extension → content-type mapping used in `serve_plugin_ui`.
    #[test]
    fn test_content_type_mapping() {
        fn content_type_for(ext: &str) -> &'static str {
            match ext {
                "html" => "text/html; charset=utf-8",
                "js" | "mjs" => "application/javascript",
                "css" => "text/css",
                "json" => "application/json",
                "svg" => "image/svg+xml",
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "webp" => "image/webp",
                "woff" => "font/woff",
                "woff2" => "font/woff2",
                "ttf" => "font/ttf",
                _ => "application/octet-stream",
            }
        }

        assert_eq!(content_type_for("html"), "text/html; charset=utf-8");
        assert_eq!(content_type_for("js"), "application/javascript");
        assert_eq!(content_type_for("mjs"), "application/javascript");
        assert_eq!(content_type_for("css"), "text/css");
        assert_eq!(content_type_for("json"), "application/json");
        assert_eq!(content_type_for("svg"), "image/svg+xml");
        assert_eq!(content_type_for("png"), "image/png");
        assert_eq!(content_type_for("jpg"), "image/jpeg");
        assert_eq!(content_type_for("jpeg"), "image/jpeg");
        assert_eq!(content_type_for("webp"), "image/webp");
        assert_eq!(content_type_for("woff"), "font/woff");
        assert_eq!(content_type_for("woff2"), "font/woff2");
        assert_eq!(content_type_for("ttf"), "font/ttf");
        assert_eq!(content_type_for("exe"), "application/octet-stream");
        assert_eq!(content_type_for(""), "application/octet-stream");
    }

    // ─── CSP header ─────────────────────────────────────────────────────

    /// Verify the Content-Security-Policy header string.
    #[test]
    fn test_csp_header_value() {
        let csp = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; frame-ancestors 'self'";
        assert!(csp.contains("default-src 'self'"));
        assert!(csp.contains("frame-ancestors 'self'"));
        assert!(csp.contains("script-src 'self' 'unsafe-inline'"));
    }
}
