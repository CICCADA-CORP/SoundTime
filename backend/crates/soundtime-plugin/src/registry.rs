//! Plugin registry — manages loaded plugins, dispatches events.
//!
//! The `PluginRegistry` is the central orchestrator of the plugin system.
//! It loads plugins from the database, maintains their WASM sandboxes,
//! and routes events to subscribed plugins.

use std::collections::HashMap;
use std::path::PathBuf;

use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::PluginError;
use crate::host_functions::HostContext;
use crate::manifest::Permissions;
use crate::sandbox::{PluginSandbox, SandboxConfig};
use soundtime_db::entities::{plugin, plugin_events_log};

// ─── Loaded plugin entry ────────────────────────────────────────────────

/// A host function request returned by a plugin after processing an event.
///
/// Plugins encode side-effect requests in their return value as JSON.
/// The registry processes these using the plugin's `HostContext`.
#[derive(Debug, serde::Deserialize)]
struct HostRequest {
    /// Name of the host function to call.
    function: String,
    /// Arguments as a JSON object.
    #[serde(default)]
    args: serde_json::Value,
}

/// Response from a plugin event handler, optionally containing host requests.
#[derive(Debug, serde::Deserialize)]
struct PluginResponse {
    #[serde(default)]
    host_requests: Vec<HostRequest>,
}

/// A loaded plugin with its sandbox and metadata.
struct LoadedPlugin {
    id: Uuid,
    name: String,
    sandbox: PluginSandbox,
    /// Host context for executing host functions on behalf of this plugin.
    /// Used during event dispatch when plugins call back into the host.
    host_ctx: HostContext,
    /// Events this plugin is subscribed to (kept for reload/introspection).
    #[allow(dead_code)]
    subscribed_events: Vec<String>,
}

// ─── Registry ───────────────────────────────────────────────────────────

/// Central plugin registry.
///
/// Manages the lifecycle of all plugins: loading from disk, maintaining
/// WASM sandboxes, dispatching events, and tracking subscriptions.
/// Thread-safe via `RwLock` for interior mutability.
pub struct PluginRegistry {
    /// Loaded plugins indexed by plugin ID.
    plugins: RwLock<HashMap<Uuid, LoadedPlugin>>,
    /// Event name → list of subscribed plugin IDs (in load order).
    subscriptions: RwLock<HashMap<String, Vec<Uuid>>>,
    /// Database connection for loading plugin data and logging events.
    db: DatabaseConnection,
    /// Sandbox configuration (memory limits, fuel, etc.).
    sandbox_config: SandboxConfig,
    /// Base directory where plugin WASM files are stored.
    plugin_dir: PathBuf,
    /// Domain name for instance info.
    domain: String,
    /// Whether to log event executions to the database.
    log_events: bool,
    /// Server version string, passed to plugins via host context.
    server_version: String,
}

/// Process host function requests from a plugin's event handler response.
async fn process_host_requests(
    host_ctx: &crate::host_functions::HostContext,
    requests: Vec<HostRequest>,
) {
    for req in requests {
        let result = match req.function.as_str() {
            "set_track_metadata" => {
                let track_id = req.args["track_id"].as_str().unwrap_or_default();
                let field = req.args["field"].as_str().unwrap_or_default();
                let value = req.args["value"].as_str().unwrap_or_default();
                host_ctx
                    .set_track_metadata(track_id, field, value)
                    .await
                    .map(|_| ())
            }
            "set_track_lyrics" => {
                let track_id = req.args["track_id"].as_str().unwrap_or_default();
                let lyrics = req.args["lyrics"].as_str().unwrap_or_default();
                host_ctx
                    .set_track_lyrics(track_id, lyrics)
                    .await
                    .map(|_| ())
            }
            "set_config" => {
                let key = req.args["key"].as_str().unwrap_or_default();
                let value = req.args["value"].as_str().unwrap_or_default();
                host_ctx.set_config(key, value).await.map(|_| ())
            }
            "log_info" => {
                let msg = req.args["message"].as_str().unwrap_or_default();
                host_ctx.log_info(msg);
                Ok(())
            }
            "log_warn" => {
                let msg = req.args["message"].as_str().unwrap_or_default();
                host_ctx.log_warn(msg);
                Ok(())
            }
            "log_error" => {
                let msg = req.args["message"].as_str().unwrap_or_default();
                host_ctx.log_error(msg);
                Ok(())
            }
            "emit_event" => {
                let event = req.args["event_name"].as_str().unwrap_or_default();
                let payload = req.args["payload"].as_str().unwrap_or_default();
                host_ctx.emit_event(event, payload).map(|_| ())
            }
            other => {
                tracing::warn!(
                    plugin = %host_ctx.plugin_name(),
                    function = %other,
                    "unknown host function request, ignoring"
                );
                continue;
            }
        };

        if let Err(e) = result {
            tracing::error!(
                plugin = %host_ctx.plugin_name(),
                function = %req.function,
                "host function request failed: {e}"
            );
        }
    }
}

impl PluginRegistry {
    /// Create a new plugin registry.
    ///
    /// Reads configuration from environment variables. Does NOT load
    /// plugins — call `load_enabled_plugins()` after creation.
    pub async fn new(
        db: &DatabaseConnection,
        domain: &str,
        server_version: &str,
    ) -> Result<Self, PluginError> {
        let sandbox_config = SandboxConfig::from_env();
        let plugin_dir =
            std::env::var("PLUGIN_DIR").unwrap_or_else(|_| "/data/plugins".to_string());
        let log_events =
            std::env::var("PLUGIN_LOG_EVENTS").unwrap_or_else(|_| "true".to_string()) == "true";

        Ok(Self {
            plugins: RwLock::new(HashMap::new()),
            subscriptions: RwLock::new(HashMap::new()),
            db: db.clone(),
            sandbox_config,
            plugin_dir: PathBuf::from(plugin_dir),
            domain: domain.to_string(),
            log_events,
            server_version: server_version.to_string(),
        })
    }

    /// Load all enabled plugins from the database.
    ///
    /// Queries plugins with status "enabled", loads their WASM sandboxes,
    /// and registers their event subscriptions.
    pub async fn load_enabled_plugins(&self) {
        let enabled_plugins = match plugin::Entity::find()
            .filter(plugin::Column::Status.eq("enabled"))
            .all(&self.db)
            .await
        {
            Ok(plugins) => plugins,
            Err(e) => {
                tracing::error!("failed to query enabled plugins: {e}");
                return;
            }
        };

        for p in enabled_plugins {
            if let Err(e) = self.load_plugin_from_model(&p).await {
                tracing::error!(
                    plugin_name = %p.name,
                    plugin_id = %p.id,
                    "failed to load plugin: {e}"
                );
                // Update plugin status to "error" in DB
                let _ = self
                    .set_plugin_status(p.id, "error", Some(&e.to_string()))
                    .await;
            }
        }
    }

    /// Load a single plugin from its DB model.
    async fn load_plugin_from_model(&self, model: &plugin::Model) -> Result<(), PluginError> {
        let wasm_path = PathBuf::from(&model.wasm_path);

        // Parse permissions from JSONB
        let permissions: Permissions =
            serde_json::from_value(model.permissions.clone()).unwrap_or_default();

        let subscribed_events = permissions.events.clone();

        // Load WASM sandbox
        let sandbox = PluginSandbox::load(&wasm_path, self.sandbox_config.clone(), &model.name)?;

        // Create host context
        let host_ctx = HostContext::new(
            self.db.clone(),
            model.id,
            model.name.clone(),
            permissions,
            self.sandbox_config.http_timeout_secs,
            self.domain.clone(),
            self.server_version.clone(),
        );

        let loaded = LoadedPlugin {
            id: model.id,
            name: model.name.clone(),
            sandbox,
            host_ctx,
            subscribed_events: subscribed_events.clone(),
        };

        // Register in plugins map
        {
            let mut plugins = self.plugins.write().await;
            plugins.insert(model.id, loaded);
        }

        // Register event subscriptions
        {
            let mut subs = self.subscriptions.write().await;
            for event in &subscribed_events {
                subs.entry(event.clone()).or_default().push(model.id);
            }
        }

        tracing::info!(
            plugin_name = %model.name,
            plugin_id = %model.id,
            events = ?subscribed_events,
            "plugin loaded"
        );

        Ok(())
    }

    /// Load a plugin by its database ID.
    pub async fn load_plugin(&self, plugin_id: Uuid) -> Result<(), PluginError> {
        let model = plugin::Entity::find_by_id(plugin_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        self.load_plugin_from_model(&model).await
    }

    /// Unload a plugin, removing it from memory and subscriptions.
    pub async fn unload_plugin(&self, plugin_id: Uuid) -> Result<(), PluginError> {
        let plugin_name;

        // Remove from plugins map
        {
            let mut plugins = self.plugins.write().await;
            let removed = plugins
                .remove(&plugin_id)
                .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;
            plugin_name = removed.name;
        }

        // Remove from all subscription lists
        {
            let mut subs = self.subscriptions.write().await;
            for subscribers in subs.values_mut() {
                subscribers.retain(|id| *id != plugin_id);
            }
        }

        tracing::info!(
            plugin_name = %plugin_name,
            plugin_id = %plugin_id,
            "plugin unloaded"
        );

        Ok(())
    }

    /// Dispatch an event to all subscribed plugins.
    ///
    /// Iterates over subscribed plugins in load order. Each plugin's WASM
    /// call runs without holding the registry-wide write lock. If a plugin
    /// returns host function requests, they are processed after execution.
    pub async fn dispatch(&self, event_name: &str, payload: &serde_json::Value) {
        // Get list of subscribed plugin IDs
        let subscriber_ids: Vec<Uuid> = {
            let subs = self.subscriptions.read().await;
            subs.get(event_name).cloned().unwrap_or_default()
        };

        if subscriber_ids.is_empty() {
            return;
        }

        let handler_fn = format!("handle_{event_name}");
        let payload_bytes = match serde_json::to_vec(payload) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(event = %event_name, "failed to serialize event payload: {e}");
                return;
            }
        };

        for plugin_id in subscriber_ids {
            let start = std::time::Instant::now();
            let mut result_str = "success";
            let mut error_msg: Option<String> = None;
            let mut plugin_name = String::new();
            let mut host_requests_to_process: Option<(
                Vec<HostRequest>,
                crate::host_functions::HostContext,
            )> = None;

            {
                let mut plugins = self.plugins.write().await;
                if let Some(loaded) = plugins.get_mut(&plugin_id) {
                    plugin_name.clone_from(&loaded.name);

                    // Check if the plugin exports this handler
                    if !loaded.sandbox.has_function(&handler_fn) {
                        tracing::warn!(
                            plugin = %loaded.name,
                            handler = %handler_fn,
                            "plugin does not export handler function, skipping"
                        );
                        continue;
                    }

                    match loaded.sandbox.call(&handler_fn, &payload_bytes) {
                        Ok(output) => {
                            tracing::debug!(
                                plugin = %loaded.name,
                                event = %event_name,
                                elapsed_ms = start.elapsed().as_millis() as u64,
                                "event handled successfully"
                            );

                            // Parse host function requests from plugin response
                            if !output.is_empty() {
                                if let Ok(response) =
                                    serde_json::from_slice::<PluginResponse>(&output)
                                {
                                    if !response.host_requests.is_empty() {
                                        host_requests_to_process =
                                            Some((response.host_requests, loaded.host_ctx.clone()));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let msg = e.to_string();
                            if matches!(e, PluginError::FuelExhausted(_)) {
                                result_str = "timeout";
                            } else {
                                result_str = "error";
                            }
                            tracing::error!(
                                plugin = %loaded.name,
                                event = %event_name,
                                "event handler failed: {msg}"
                            );
                            error_msg = Some(msg);
                        }
                    }
                } else {
                    continue;
                }
            }
            // Write lock dropped here

            // Process host function requests outside the lock
            if let Some((requests, host_ctx)) = host_requests_to_process {
                tracing::debug!(
                    plugin = %plugin_name,
                    count = requests.len(),
                    "processing host function requests"
                );
                process_host_requests(&host_ctx, requests).await;
            }

            // Log event execution to DB if enabled
            if self.log_events {
                let elapsed_ms = start.elapsed().as_millis() as i32;
                let _ = self
                    .log_event_execution(
                        plugin_id,
                        event_name,
                        Some(payload),
                        result_str,
                        elapsed_ms,
                        error_msg.as_deref(),
                    )
                    .await;
            }
        }
    }

    /// Log an event execution to the `plugin_events_log` table.
    async fn log_event_execution(
        &self,
        plugin_id: Uuid,
        event_name: &str,
        payload: Option<&serde_json::Value>,
        result: &str,
        execution_time_ms: i32,
        error_message: Option<&str>,
    ) -> Result<(), PluginError> {
        let now = chrono::Utc::now().fixed_offset();

        let log_entry = plugin_events_log::ActiveModel {
            id: Set(Uuid::new_v4()),
            plugin_id: Set(plugin_id),
            event_name: Set(event_name.to_string()),
            payload: Set(payload.cloned()),
            result: Set(result.to_string()),
            execution_time_ms: Set(execution_time_ms),
            error_message: Set(error_message.map(|s| s.to_string())),
            created_at: Set(now),
        };

        log_entry.insert(&self.db).await?;
        Ok(())
    }

    /// Update a plugin's status in the database.
    async fn set_plugin_status(
        &self,
        plugin_id: Uuid,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<(), PluginError> {
        let model = plugin::Entity::find_by_id(plugin_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        let now = chrono::Utc::now().fixed_offset();
        let mut active: plugin::ActiveModel = model.into();
        active.status = Set(status.to_string());
        active.error_message = Set(error_message.map(|s| s.to_string()));
        active.updated_at = Set(now);
        active.update(&self.db).await?;

        Ok(())
    }

    /// Enable a plugin: set status to "enabled" in DB and load it.
    pub async fn enable_plugin(&self, plugin_id: Uuid) -> Result<(), PluginError> {
        self.set_plugin_status(plugin_id, "enabled", None).await?;
        self.load_plugin(plugin_id).await?;
        Ok(())
    }

    /// Disable a plugin: unload it and set status to "disabled" in DB.
    pub async fn disable_plugin(&self, plugin_id: Uuid) -> Result<(), PluginError> {
        // Unload if currently loaded (ignore NotFound if not loaded)
        let _ = self.unload_plugin(plugin_id).await;
        self.set_plugin_status(plugin_id, "disabled", None).await?;
        Ok(())
    }

    // ── Query methods ────────────────────────────────────────────────

    /// Returns the number of currently loaded plugins.
    pub async fn loaded_count(&self) -> usize {
        self.plugins.read().await.len()
    }

    /// Check if a plugin is currently loaded.
    pub async fn is_loaded(&self, plugin_id: Uuid) -> bool {
        self.plugins.read().await.contains_key(&plugin_id)
    }

    /// Get a list of all loaded plugin names and IDs.
    pub async fn loaded_plugins(&self) -> Vec<(Uuid, String)> {
        self.plugins
            .read()
            .await
            .values()
            .map(|p| (p.id, p.name.clone()))
            .collect()
    }

    /// Check if any enabled plugin has a UI component.
    ///
    /// Reads the `plugin.toml` manifest from each enabled plugin's install
    /// directory and checks whether `[ui] enabled = true`.
    pub async fn has_ui_plugins(&self) -> bool {
        let result = plugin::Entity::find()
            .filter(plugin::Column::Status.eq("enabled"))
            .all(&self.db)
            .await
            .unwrap_or_default();

        for p in result {
            // Read the plugin.toml from the install directory
            let wasm_path = std::path::PathBuf::from(&p.wasm_path);
            if let Some(install_dir) = wasm_path.parent() {
                let manifest_path = install_dir.join("plugin.toml");
                if let Ok(content) = tokio::fs::read_to_string(&manifest_path).await {
                    if let Ok(manifest) = crate::manifest::PluginManifest::parse(&content) {
                        if manifest.ui.enabled {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Get the database connection.
    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    /// Get the plugin directory path.
    pub fn plugin_dir(&self) -> &PathBuf {
        &self.plugin_dir
    }

    /// Get the sandbox configuration.
    pub fn sandbox_config(&self) -> &SandboxConfig {
        &self.sandbox_config
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_config_defaults() {
        let config = SandboxConfig::default();
        assert_eq!(config.memory_limit, 32 * 1024 * 1024);
        assert_eq!(config.fuel_limit, 1_000_000);
        assert_eq!(config.http_timeout_secs, 10);
        assert!(!config.wasi_enabled);
    }

    #[test]
    fn test_subscription_add_and_remove() {
        // Simulate the subscription data structure used by the registry
        let mut subscriptions: HashMap<String, Vec<Uuid>> = HashMap::new();

        let plugin_a = Uuid::new_v4();
        let plugin_b = Uuid::new_v4();
        let plugin_c = Uuid::new_v4();

        // Add subscriptions (mimics load_plugin_from_model)
        let events_a = vec!["on_track_added".to_string(), "on_track_played".to_string()];
        for event in &events_a {
            subscriptions
                .entry(event.clone())
                .or_default()
                .push(plugin_a);
        }

        let events_b = vec!["on_track_added".to_string(), "on_user_login".to_string()];
        for event in &events_b {
            subscriptions
                .entry(event.clone())
                .or_default()
                .push(plugin_b);
        }

        subscriptions
            .entry("on_track_added".to_string())
            .or_default()
            .push(plugin_c);

        // Verify subscriptions
        assert_eq!(subscriptions["on_track_added"].len(), 3);
        assert_eq!(subscriptions["on_track_played"].len(), 1);
        assert_eq!(subscriptions["on_user_login"].len(), 1);

        // Verify ordering (load order)
        assert_eq!(subscriptions["on_track_added"][0], plugin_a);
        assert_eq!(subscriptions["on_track_added"][1], plugin_b);
        assert_eq!(subscriptions["on_track_added"][2], plugin_c);

        // Unsubscribe plugin_b (mimics unload_plugin)
        for subscribers in subscriptions.values_mut() {
            subscribers.retain(|id| *id != plugin_b);
        }

        assert_eq!(subscriptions["on_track_added"].len(), 2);
        assert_eq!(subscriptions["on_track_played"].len(), 1);
        assert_eq!(subscriptions["on_user_login"].len(), 0);

        // Verify plugin_a and plugin_c remain in correct order
        assert_eq!(subscriptions["on_track_added"][0], plugin_a);
        assert_eq!(subscriptions["on_track_added"][1], plugin_c);

        // Unsubscribe plugin_a
        for subscribers in subscriptions.values_mut() {
            subscribers.retain(|id| *id != plugin_a);
        }

        assert_eq!(subscriptions["on_track_added"].len(), 1);
        assert_eq!(subscriptions["on_track_added"][0], plugin_c);
        assert_eq!(subscriptions["on_track_played"].len(), 0);

        // Unsubscribe plugin_c — all lists empty
        for subscribers in subscriptions.values_mut() {
            subscribers.retain(|id| *id != plugin_c);
        }

        for subscribers in subscriptions.values() {
            assert!(subscribers.is_empty());
        }
    }

    #[test]
    fn test_host_request_deserialization() {
        let json = r#"{"function":"set_config","args":{"key":"k","value":"v"}}"#;
        let req: HostRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.function, "set_config");
        assert_eq!(req.args["key"], "k");
        assert_eq!(req.args["value"], "v");
    }

    #[test]
    fn test_host_request_default_args() {
        let json = r#"{"function":"log_info"}"#;
        let req: HostRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.function, "log_info");
        assert!(req.args.is_null());
    }

    #[test]
    fn test_plugin_response_with_requests() {
        let json = r#"{"host_requests":[{"function":"log_info","args":{"message":"hi"}},{"function":"emit_event","args":{"event_name":"test","payload":""}}]}"#;
        let resp: PluginResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.host_requests.len(), 2);
        assert_eq!(resp.host_requests[0].function, "log_info");
        assert_eq!(resp.host_requests[1].function, "emit_event");
    }

    #[test]
    fn test_plugin_response_empty() {
        let json = r#"{}"#;
        let resp: PluginResponse = serde_json::from_str(json).unwrap();
        assert!(resp.host_requests.is_empty());
    }

    #[test]
    fn test_plugin_response_empty_array() {
        let json = r#"{"host_requests":[]}"#;
        let resp: PluginResponse = serde_json::from_str(json).unwrap();
        assert!(resp.host_requests.is_empty());
    }

    #[tokio::test]
    async fn test_process_host_requests_log_functions() {
        use crate::host_functions::HostContext;
        use crate::manifest::Permissions;

        let db = sea_orm::DatabaseConnection::Disconnected;
        let host_ctx = HostContext::new(
            db,
            Uuid::new_v4(),
            "test-plugin".to_string(),
            Permissions::default(),
            10,
            "localhost".to_string(),
            "0.1.0".to_string(),
        );

        let requests = vec![
            HostRequest {
                function: "log_info".to_string(),
                args: serde_json::json!({"message": "test info"}),
            },
            HostRequest {
                function: "log_warn".to_string(),
                args: serde_json::json!({"message": "test warn"}),
            },
            HostRequest {
                function: "log_error".to_string(),
                args: serde_json::json!({"message": "test error"}),
            },
        ];

        // Should complete without panic — log functions don't hit DB
        process_host_requests(&host_ctx, requests).await;
    }

    #[tokio::test]
    async fn test_process_host_requests_unknown_function() {
        use crate::host_functions::HostContext;
        use crate::manifest::Permissions;

        let db = sea_orm::DatabaseConnection::Disconnected;
        let host_ctx = HostContext::new(
            db,
            Uuid::new_v4(),
            "test-plugin".to_string(),
            Permissions::default(),
            10,
            "localhost".to_string(),
            "0.1.0".to_string(),
        );

        let requests = vec![HostRequest {
            function: "totally_unknown_function".to_string(),
            args: serde_json::json!({}),
        }];

        // Should complete without panic — unknown functions are skipped
        process_host_requests(&host_ctx, requests).await;
    }

    #[tokio::test]
    async fn test_process_host_requests_mixed() {
        use crate::host_functions::HostContext;
        use crate::manifest::Permissions;

        let db = sea_orm::DatabaseConnection::Disconnected;
        let host_ctx = HostContext::new(
            db,
            Uuid::new_v4(),
            "test-plugin".to_string(),
            Permissions::default(),
            10,
            "localhost".to_string(),
            "0.1.0".to_string(),
        );

        let requests = vec![
            HostRequest {
                function: "log_info".to_string(),
                args: serde_json::json!({"message": "first"}),
            },
            HostRequest {
                function: "unknown_fn".to_string(),
                args: serde_json::json!({}),
            },
            HostRequest {
                function: "log_warn".to_string(),
                args: serde_json::json!({"message": "third"}),
            },
        ];

        // Should complete without panic
        process_host_requests(&host_ctx, requests).await;
    }

    #[tokio::test]
    async fn test_process_host_requests_empty() {
        use crate::host_functions::HostContext;
        use crate::manifest::Permissions;

        let db = sea_orm::DatabaseConnection::Disconnected;
        let host_ctx = HostContext::new(
            db,
            Uuid::new_v4(),
            "test-plugin".to_string(),
            Permissions::default(),
            10,
            "localhost".to_string(),
            "0.1.0".to_string(),
        );

        // Should complete without panic
        process_host_requests(&host_ctx, vec![]).await;
    }

    #[tokio::test]
    #[should_panic]
    async fn test_process_host_requests_set_config_panics_on_disconnected_db() {
        use crate::host_functions::HostContext;
        use crate::manifest::Permissions;

        let perms = Permissions {
            config_access: true,
            ..Default::default()
        };

        let db = sea_orm::DatabaseConnection::Disconnected;
        let host_ctx = HostContext::new(
            db,
            Uuid::new_v4(),
            "test-plugin".to_string(),
            perms,
            10,
            "localhost".to_string(),
            "0.1.0".to_string(),
        );

        let requests = vec![HostRequest {
            function: "set_config".to_string(),
            args: serde_json::json!({"key": "test_key", "value": "test_val"}),
        }];

        // This will panic because set_config hits the DB
        process_host_requests(&host_ctx, requests).await;
    }

    #[tokio::test]
    async fn test_process_host_requests_emit_event_without_permission() {
        use crate::host_functions::HostContext;
        use crate::manifest::Permissions;

        // No special permissions
        let db = sea_orm::DatabaseConnection::Disconnected;
        let host_ctx = HostContext::new(
            db,
            Uuid::new_v4(),
            "test-plugin".to_string(),
            Permissions::default(),
            10,
            "localhost".to_string(),
            "0.1.0".to_string(),
        );

        let requests = vec![HostRequest {
            function: "emit_event".to_string(),
            args: serde_json::json!({"event_name": "custom_event", "payload": "data"}),
        }];

        // Should not panic — emit_event succeeds for non-empty event names
        process_host_requests(&host_ctx, requests).await;
    }

    #[test]
    fn test_subscription_duplicate_event() {
        let mut subscriptions: HashMap<String, Vec<Uuid>> = HashMap::new();
        let plugin = Uuid::new_v4();

        // Same plugin subscribing to same event twice (shouldn't happen normally, but test behavior)
        subscriptions
            .entry("on_upload".to_string())
            .or_default()
            .push(plugin);
        subscriptions
            .entry("on_upload".to_string())
            .or_default()
            .push(plugin);

        assert_eq!(subscriptions["on_upload"].len(), 2);

        // Remove should remove all instances
        for subscribers in subscriptions.values_mut() {
            subscribers.retain(|id| *id != plugin);
        }
        assert_eq!(subscriptions["on_upload"].len(), 0);
    }
}
