//! WASM plugin sandbox using Extism (wasmtime).
//!
//! Each plugin runs in its own isolated WASM sandbox with configurable
//! memory limits and fuel-based execution limits.

use std::path::Path;

use serde::{de::DeserializeOwned, Serialize};

use crate::error::PluginError;

// ─── Configuration ──────────────────────────────────────────────────────

/// Configuration for the WASM sandbox.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Maximum memory in bytes (default: 32 MB).
    pub memory_limit: usize,
    /// Maximum fuel (instructions) per execution (default: 1_000_000).
    pub fuel_limit: u64,
    /// HTTP request timeout in seconds (default: 10).
    pub http_timeout_secs: u64,
    /// Whether to enable WASI (default: false for security).
    /// When false, plugins cannot access env vars, filesystem, or stdio.
    pub wasi_enabled: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            memory_limit: 32 * 1024 * 1024,
            fuel_limit: 1_000_000,
            http_timeout_secs: 10,
            wasi_enabled: false,
        }
    }
}

impl SandboxConfig {
    /// Build config from environment variables.
    pub fn from_env() -> Self {
        Self {
            memory_limit: std::env::var("PLUGIN_MEMORY_LIMIT_MB")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(32)
                * 1024
                * 1024,
            fuel_limit: std::env::var("PLUGIN_FUEL_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1_000_000),
            http_timeout_secs: std::env::var("PLUGIN_HTTP_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            wasi_enabled: std::env::var("PLUGIN_WASI_ENABLED")
                .unwrap_or_default()
                .eq_ignore_ascii_case("true"),
        }
    }
}

// ─── Sandbox ────────────────────────────────────────────────────────────

/// A loaded WASM plugin sandbox.
///
/// Wraps an Extism plugin with memory limits and fuel-based execution
/// limits. Each plugin call gets a fresh fuel budget.
pub struct PluginSandbox {
    plugin: extism::Plugin,
    config: SandboxConfig,
    plugin_name: String,
}

impl std::fmt::Debug for PluginSandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginSandbox")
            .field("plugin_name", &self.plugin_name)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl PluginSandbox {
    /// Load a WASM plugin from disk into a sandboxed environment.
    ///
    /// Reads the WASM binary, configures memory limits (in 64 KB pages),
    /// and enables fuel-based execution limits via the `PluginBuilder`.
    pub fn load(wasm_path: &Path, config: SandboxConfig, name: &str) -> Result<Self, PluginError> {
        let wasm_bytes = std::fs::read(wasm_path)?;

        let manifest = extism::Manifest::new([extism::Wasm::data(wasm_bytes)])
            .with_memory_max((config.memory_limit / 65536) as u32);

        let builder = extism::PluginBuilder::new(manifest)
            .with_wasi(config.wasi_enabled)
            .with_fuel_limit(config.fuel_limit);

        if config.wasi_enabled {
            tracing::warn!(
                plugin = %name,
                "WASI enabled for plugin — plugin can access environment variables. \
                 Avoid running with sensitive env vars exposed."
            );
        }

        let plugin = builder
            .build()
            .map_err(|e| PluginError::Sandbox(e.to_string()))?;

        Ok(Self {
            plugin,
            config,
            plugin_name: name.to_string(),
        })
    }

    /// Call a WASM function by name with raw byte input/output.
    ///
    /// Sets a fresh fuel budget before each call. Errors are classified
    /// into fuel exhaustion, memory exceeded, or general sandbox errors.
    pub fn call(&mut self, function_name: &str, input: &[u8]) -> Result<Vec<u8>, PluginError> {
        // Fuel is reset per call via PluginBuilder's fuel_limit configuration.
        // The extism Plugin internally resets fuel at the start of each raw_call
        // when fuel was configured via PluginBuilder::with_fuel_limit.

        let output = self
            .plugin
            .call::<&[u8], Vec<u8>>(function_name, input)
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("fuel") {
                    PluginError::FuelExhausted(self.plugin_name.clone())
                } else if msg.contains("memory") {
                    PluginError::MemoryExceeded(self.plugin_name.clone())
                } else {
                    PluginError::Sandbox(msg)
                }
            })?;

        Ok(output)
    }

    /// Call a WASM function with JSON-serialized input and output.
    ///
    /// This is the main API used by the event system. Input is serialized
    /// to JSON bytes, passed to the plugin, and the output is deserialized
    /// back from JSON.
    pub fn call_json<I: Serialize, O: DeserializeOwned>(
        &mut self,
        function_name: &str,
        input: &I,
    ) -> Result<O, PluginError> {
        let json_bytes = serde_json::to_vec(input)?;
        let output_bytes = self.call(function_name, &json_bytes)?;
        let result = serde_json::from_slice(&output_bytes)?;
        Ok(result)
    }

    /// Check if the plugin exports a function with the given name.
    pub fn has_function(&self, name: &str) -> bool {
        self.plugin.function_exists(name)
    }

    /// Returns the plugin name.
    pub fn name(&self) -> &str {
        &self.plugin_name
    }

    /// Returns a reference to the sandbox configuration.
    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_sandbox_config_default() {
        let config = SandboxConfig::default();
        assert_eq!(config.memory_limit, 32 * 1024 * 1024);
        assert_eq!(config.fuel_limit, 1_000_000);
        assert_eq!(config.http_timeout_secs, 10);
        assert!(!config.wasi_enabled);
    }

    #[test]
    fn test_sandbox_config_from_env() {
        std::env::set_var("PLUGIN_MEMORY_LIMIT_MB", "64");
        std::env::set_var("PLUGIN_FUEL_LIMIT", "2000000");
        std::env::set_var("PLUGIN_HTTP_TIMEOUT_SECS", "30");
        std::env::set_var("PLUGIN_WASI_ENABLED", "true");

        let config = SandboxConfig::from_env();
        assert_eq!(config.memory_limit, 64 * 1024 * 1024);
        assert_eq!(config.fuel_limit, 2_000_000);
        assert_eq!(config.http_timeout_secs, 30);
        assert!(config.wasi_enabled);

        // Clean up
        std::env::remove_var("PLUGIN_MEMORY_LIMIT_MB");
        std::env::remove_var("PLUGIN_FUEL_LIMIT");
        std::env::remove_var("PLUGIN_HTTP_TIMEOUT_SECS");
        std::env::remove_var("PLUGIN_WASI_ENABLED");

        // Verify default (unset) is false
        let config_default = SandboxConfig::from_env();
        assert!(!config_default.wasi_enabled);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let config = SandboxConfig::default();
        let result = PluginSandbox::load(Path::new("/nonexistent/plugin.wasm"), config, "test");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, PluginError::Io(_)),
            "expected Io error, got: {err:?}"
        );
    }

    #[test]
    fn test_load_invalid_wasm() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let wasm_path = dir.path().join("bad.wasm");
        let mut f = std::fs::File::create(&wasm_path).expect("failed to create temp file");
        f.write_all(b"this is not valid wasm at all")
            .expect("failed to write");
        drop(f);

        let config = SandboxConfig::default();
        let result = PluginSandbox::load(&wasm_path, config, "bad-plugin");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, PluginError::Sandbox(_)),
            "expected Sandbox error, got: {err:?}"
        );
    }
}
