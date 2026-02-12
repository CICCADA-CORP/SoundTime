//! Plugin manifest parsing and validation.
//!
//! Parses `plugin.toml` files that declare plugin metadata, permissions,
//! and build configuration.

use serde::{Deserialize, Serialize};

use crate::error::PluginError;
use crate::events::KNOWN_EVENTS;

/// Valid UI slot names where plugins can inject frontend panels.
pub const VALID_UI_SLOTS: &[&str] = &[
    "track-detail-sidebar",
    "player-extra-controls",
    "library-toolbar",
    "settings-panel",
];

/// Plugin manifest parsed from `plugin.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    pub build: BuildConfig,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub ui: UiConfig,
}

/// Plugin metadata section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub min_soundtime_version: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
}

/// Build configuration section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    pub wasm: String,
}

/// Plugin permissions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Permissions {
    #[serde(default)]
    pub http_hosts: Vec<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub write_tracks: bool,
    #[serde(default)]
    pub config_access: bool,
    #[serde(default)]
    pub read_users: bool,
}

/// UI configuration for plugins with a frontend panel.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub slot: Option<String>,
    #[serde(default)]
    pub entry: Option<String>,
}

// ─── Validation helpers ─────────────────────────────────────────────

/// Validate a plugin name against `^[a-z][a-z0-9-]{1,63}$`.
///
/// The name must start with a lowercase ASCII letter, followed by 1-63
/// characters that are lowercase ASCII letters, digits, or hyphens.
/// Total length: 2-64 characters.
fn validate_plugin_name(name: &str) -> Result<(), PluginError> {
    let len = name.len();
    if !(2..=64).contains(&len) {
        return Err(PluginError::InvalidManifest(format!(
            "plugin name must be 2-64 characters, got {len}"
        )));
    }

    let mut chars = name.chars();

    // First character must be a lowercase ASCII letter
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() {
        return Err(PluginError::InvalidManifest(format!(
            "plugin name must start with a lowercase letter, got '{first}'"
        )));
    }

    // Remaining characters must be lowercase letters, digits, or hyphens
    for ch in chars {
        if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '-' {
            return Err(PluginError::InvalidManifest(format!(
                "plugin name contains invalid character '{ch}'"
            )));
        }
    }

    Ok(())
}

/// Validate a version string as semver.
fn validate_semver(value: &str, field_name: &str) -> Result<(), PluginError> {
    semver::Version::parse(value).map_err(|_| {
        PluginError::InvalidManifest(format!("{field_name} is not valid semver: '{value}'"))
    })?;
    Ok(())
}

/// Validate that a path is safe (no `..` components, not absolute).
fn validate_path_safety(path: &str, field_name: &str) -> Result<(), PluginError> {
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        return Err(PluginError::InvalidManifest(format!(
            "{field_name} must be a relative path, got absolute: '{path}'"
        )));
    }
    for component in p.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(PluginError::InvalidManifest(format!(
                "{field_name} must not contain '..': '{path}'"
            )));
        }
    }
    Ok(())
}

/// Validate an HTTP host entry.
///
/// Must be non-empty, contain no spaces, and either be `"*"`, `"localhost"`,
/// or contain at least one dot (basic domain validation).
fn validate_http_host(host: &str) -> Result<(), PluginError> {
    if host.is_empty() {
        return Err(PluginError::InvalidManifest(
            "http_hosts entry must not be empty".into(),
        ));
    }
    if host.contains(' ') {
        return Err(PluginError::InvalidManifest(format!(
            "http_hosts entry must not contain spaces: '{host}'"
        )));
    }
    if host != "*" && host != "localhost" && !host.contains('.') {
        return Err(PluginError::InvalidManifest(format!(
            "http_hosts entry is not a valid domain: '{host}'"
        )));
    }
    Ok(())
}

impl PluginManifest {
    /// Parse a plugin manifest from a TOML string.
    pub fn parse(toml_str: &str) -> Result<Self, PluginError> {
        let manifest: PluginManifest = toml::from_str(toml_str)?;
        Ok(manifest)
    }

    /// Validate all fields of a parsed manifest.
    pub fn validate(&self) -> Result<(), PluginError> {
        // ── Plugin metadata ─────────────────────────────────────────
        validate_plugin_name(&self.plugin.name)?;

        validate_semver(&self.plugin.version, "plugin.version")?;

        let desc_len = self.plugin.description.len();
        if desc_len == 0 || desc_len > 500 {
            return Err(PluginError::InvalidManifest(format!(
                "plugin.description must be 1-500 characters, got {desc_len}"
            )));
        }

        if let Some(ref author) = self.plugin.author {
            let len = author.len();
            if len == 0 || len > 255 {
                return Err(PluginError::InvalidManifest(format!(
                    "plugin.author must be 1-255 characters, got {len}"
                )));
            }
        }

        if let Some(ref license) = self.plugin.license {
            let len = license.len();
            if len == 0 || len > 50 {
                return Err(PluginError::InvalidManifest(format!(
                    "plugin.license must be 1-50 characters, got {len}"
                )));
            }
        }

        if let Some(ref min_ver) = self.plugin.min_soundtime_version {
            validate_semver(min_ver, "plugin.min_soundtime_version")?;
        }

        // ── Build config ────────────────────────────────────────────
        validate_path_safety(&self.build.wasm, "build.wasm")?;

        if !self.build.wasm.ends_with(".wasm") {
            return Err(PluginError::InvalidManifest(format!(
                "build.wasm must end with '.wasm', got '{}'",
                self.build.wasm
            )));
        }

        // ── Permissions ─────────────────────────────────────────────
        for event in &self.permissions.events {
            if !KNOWN_EVENTS.contains(&event.as_str()) {
                return Err(PluginError::InvalidManifest(format!(
                    "unknown event '{event}'; known events: {}",
                    KNOWN_EVENTS.join(", ")
                )));
            }
        }

        for host in &self.permissions.http_hosts {
            validate_http_host(host)?;
        }

        // ── UI config ───────────────────────────────────────────────
        if self.ui.enabled {
            let slot = self.ui.slot.as_deref().ok_or_else(|| {
                PluginError::InvalidManifest("ui.slot is required when ui.enabled is true".into())
            })?;

            if !VALID_UI_SLOTS.contains(&slot) {
                return Err(PluginError::InvalidManifest(format!(
                    "invalid ui.slot '{slot}'; valid slots: {}",
                    VALID_UI_SLOTS.join(", ")
                )));
            }

            let entry = self.ui.entry.as_deref().ok_or_else(|| {
                PluginError::InvalidManifest("ui.entry is required when ui.enabled is true".into())
            })?;

            validate_path_safety(entry, "ui.entry")?;

            if !entry.ends_with(".html") {
                return Err(PluginError::InvalidManifest(format!(
                    "ui.entry must end with '.html', got '{entry}'"
                )));
            }
        } else if let Some(ref entry) = self.ui.entry {
            validate_path_safety(entry, "ui.entry")?;

            if !entry.ends_with(".html") {
                return Err(PluginError::InvalidManifest(format!(
                    "ui.entry must end with '.html', got '{entry}'"
                )));
            }
        }

        Ok(())
    }

    /// Parse and validate a plugin manifest from a TOML string.
    pub fn parse_and_validate(toml_str: &str) -> Result<Self, PluginError> {
        let manifest = Self::parse(toml_str)?;
        manifest.validate()?;
        Ok(manifest)
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Full valid TOML manifest with all fields populated.
    const FULL_VALID_TOML: &str = r#"
[plugin]
name = "my-cool-plugin"
version = "1.2.3"
description = "A cool plugin for SoundTime"
author = "Jane Doe"
license = "MIT"
min_soundtime_version = "0.5.0"
homepage = "https://example.com"

[build]
wasm = "target/plugin.wasm"

[permissions]
http_hosts = ["api.example.com", "*.cdn.example.com"]
events = ["on_track_added", "on_track_played"]
write_tracks = true
config_access = false
read_users = false

[ui]
enabled = true
slot = "track-detail-sidebar"
entry = "ui/panel.html"
"#;

    /// Minimal valid TOML with only required fields.
    const MINIMAL_VALID_TOML: &str = r#"
[plugin]
name = "ab"
version = "0.1.0"
description = "Minimal plugin"

[build]
wasm = "plugin.wasm"
"#;

    // ── Parsing ─────────────────────────────────────────────────────

    #[test]
    fn test_parse_valid_manifest() {
        let manifest = PluginManifest::parse(FULL_VALID_TOML).unwrap();
        assert_eq!(manifest.plugin.name, "my-cool-plugin");
        assert_eq!(manifest.plugin.version, "1.2.3");
        assert_eq!(manifest.plugin.description, "A cool plugin for SoundTime");
        assert_eq!(manifest.plugin.author.as_deref(), Some("Jane Doe"));
        assert_eq!(manifest.plugin.license.as_deref(), Some("MIT"));
        assert_eq!(
            manifest.plugin.min_soundtime_version.as_deref(),
            Some("0.5.0")
        );
        assert_eq!(
            manifest.plugin.homepage.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(manifest.build.wasm, "target/plugin.wasm");
        assert_eq!(
            manifest.permissions.http_hosts,
            vec!["api.example.com", "*.cdn.example.com"]
        );
        assert_eq!(
            manifest.permissions.events,
            vec!["on_track_added", "on_track_played"]
        );
        assert!(manifest.permissions.write_tracks);
        assert!(!manifest.permissions.config_access);
        assert!(!manifest.permissions.read_users);
        assert!(manifest.ui.enabled);
        assert_eq!(manifest.ui.slot.as_deref(), Some("track-detail-sidebar"));
        assert_eq!(manifest.ui.entry.as_deref(), Some("ui/panel.html"));
    }

    #[test]
    fn test_parse_minimal_manifest() {
        let manifest = PluginManifest::parse(MINIMAL_VALID_TOML).unwrap();
        assert_eq!(manifest.plugin.name, "ab");
        assert_eq!(manifest.plugin.version, "0.1.0");
        assert_eq!(manifest.plugin.description, "Minimal plugin");
        assert!(manifest.plugin.author.is_none());
        assert!(manifest.plugin.license.is_none());
        assert!(manifest.plugin.min_soundtime_version.is_none());
        assert!(manifest.plugin.homepage.is_none());
        assert_eq!(manifest.build.wasm, "plugin.wasm");
        assert!(manifest.permissions.http_hosts.is_empty());
        assert!(manifest.permissions.events.is_empty());
        assert!(!manifest.permissions.write_tracks);
        assert!(!manifest.permissions.config_access);
        assert!(!manifest.permissions.read_users);
        assert!(!manifest.ui.enabled);
        assert!(manifest.ui.slot.is_none());
        assert!(manifest.ui.entry.is_none());
    }

    // ── Name validation ─────────────────────────────────────────────

    #[test]
    fn test_validate_invalid_name_uppercase() {
        let toml = r#"
[plugin]
name = "MyPlugin"
version = "1.0.0"
description = "Bad name"
[build]
wasm = "p.wasm"
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(
            err.to_string().contains("uppercase")
                || err.to_string().contains("invalid character")
                || err.to_string().contains("lowercase")
        );
    }

    #[test]
    fn test_validate_invalid_name_too_short() {
        // Empty name won't parse into a valid struct easily, test single char
        let toml = r#"
[plugin]
name = "a"
version = "1.0.0"
description = "Too short"
[build]
wasm = "p.wasm"
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains("2-64 characters"));

        // Also test empty string
        let toml_empty = r#"
[plugin]
name = ""
version = "1.0.0"
description = "Empty"
[build]
wasm = "p.wasm"
"#;
        let manifest = PluginManifest::parse(toml_empty).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains("2-64 characters"));
    }

    // ── Version validation ──────────────────────────────────────────

    #[test]
    fn test_validate_invalid_version() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "not.a.version"
description = "Bad version"
[build]
wasm = "p.wasm"
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains("semver"));
    }

    // ── Description validation ──────────────────────────────────────

    #[test]
    fn test_validate_invalid_description_empty() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = ""
[build]
wasm = "p.wasm"
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains("1-500 characters"));
    }

    // ── Build config validation ─────────────────────────────────────

    #[test]
    fn test_validate_invalid_wasm_extension() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = "Bad wasm"
[build]
wasm = "plugin.js"
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains(".wasm"));
    }

    // ── Event validation ────────────────────────────────────────────

    #[test]
    fn test_validate_unknown_event() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = "Unknown event"
[build]
wasm = "p.wasm"
[permissions]
events = ["on_foo_bar"]
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains("on_foo_bar"));
    }

    #[test]
    fn test_validate_valid_events() {
        let events_toml: String = KNOWN_EVENTS
            .iter()
            .map(|e| format!("\"{e}\""))
            .collect::<Vec<_>>()
            .join(", ");

        let toml = format!(
            r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = "All events"
[build]
wasm = "p.wasm"
[permissions]
events = [{events_toml}]
"#
        );

        let manifest = PluginManifest::parse_and_validate(&toml).unwrap();
        assert_eq!(manifest.permissions.events.len(), KNOWN_EVENTS.len());
    }

    // ── UI validation ───────────────────────────────────────────────

    #[test]
    fn test_validate_ui_enabled_missing_slot() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = "Missing slot"
[build]
wasm = "p.wasm"
[ui]
enabled = true
entry = "panel.html"
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains("ui.slot"));
    }

    #[test]
    fn test_validate_ui_enabled_missing_entry() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = "Missing entry"
[build]
wasm = "p.wasm"
[ui]
enabled = true
slot = "settings-panel"
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains("ui.entry"));
    }

    #[test]
    fn test_validate_ui_invalid_slot() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = "Invalid slot"
[build]
wasm = "p.wasm"
[ui]
enabled = true
slot = "nonexistent-slot"
entry = "panel.html"
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains("nonexistent-slot"));
    }

    // ── HTTP hosts validation ───────────────────────────────────────

    #[test]
    fn test_validate_http_hosts_wildcard() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = "Wildcard host"
[build]
wasm = "p.wasm"
[permissions]
http_hosts = ["*"]
"#;
        let manifest = PluginManifest::parse_and_validate(toml).unwrap();
        assert_eq!(manifest.permissions.http_hosts, vec!["*"]);
    }

    #[test]
    fn test_validate_http_hosts_valid_domain() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = "Valid domain"
[build]
wasm = "p.wasm"
[permissions]
http_hosts = ["api.example.com"]
"#;
        let manifest = PluginManifest::parse_and_validate(toml).unwrap();
        assert_eq!(manifest.permissions.http_hosts, vec!["api.example.com"]);
    }

    #[test]
    fn test_validate_http_hosts_empty_string() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = "Empty host"
[build]
wasm = "p.wasm"
[permissions]
http_hosts = [""]
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains("empty"));
    }

    // ── parse_and_validate ──────────────────────────────────────────

    #[test]
    fn test_parse_and_validate_valid() {
        let manifest = PluginManifest::parse_and_validate(FULL_VALID_TOML).unwrap();
        assert_eq!(manifest.plugin.name, "my-cool-plugin");
        assert_eq!(manifest.plugin.version, "1.2.3");
        assert!(manifest.ui.enabled);
    }

    // ── TOML parse errors ───────────────────────────────────────────

    #[test]
    fn test_parse_invalid_toml() {
        let err = PluginManifest::parse("this is not valid {{{{ toml").unwrap_err();
        assert!(matches!(err, PluginError::TomlParse(_)));
    }

    // ── Path traversal validation ────────────────────────────────────

    #[test]
    fn test_validate_path_traversal_wasm() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = "Path traversal"
[build]
wasm = "../../etc/plugin.wasm"
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains(".."));
    }

    #[test]
    fn test_validate_absolute_path_wasm() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = "Absolute path"
[build]
wasm = "/etc/plugin.wasm"
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains("absolute"));
    }

    #[test]
    fn test_validate_path_traversal_ui_entry() {
        let toml = r#"
[plugin]
name = "my-plugin"
version = "1.0.0"
description = "UI path traversal"
[build]
wasm = "plugin.wasm"
[ui]
enabled = true
slot = "settings-panel"
entry = "../../../etc/panel.html"
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        let err = manifest.validate().unwrap_err();
        assert!(matches!(err, PluginError::InvalidManifest(_)));
        assert!(err.to_string().contains(".."));
    }
}
