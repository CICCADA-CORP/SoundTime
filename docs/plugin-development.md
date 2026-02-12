# Plugin Development Guide

SoundTime supports extensibility through a WASM-based plugin system. This guide covers everything you need to build, test, and distribute plugins for SoundTime.

## Introduction

### What plugins can do

Plugins extend SoundTime's functionality without modifying the core codebase. Common use cases include:

- **Metadata enrichment** — Fetch lyrics, genres, or album art from external APIs
- **Scrobbling** — Submit listening activity to services like ListenBrainz or Last.fm
- **Webhooks** — Relay system events to external endpoints (Slack, Discord, custom services)
- **UI panels** — Add custom interface elements to the track detail page, player, library, or settings
- **Inter-plugin communication** — Emit and consume custom events between plugins

### Architecture overview

Plugins are compiled as [WebAssembly](https://webassembly.org/) (WASM) modules and executed inside sandboxed runtimes powered by [Extism](https://extism.org/) (built on [wasmtime](https://wasmtime.dev/)). Each plugin runs in its own isolated memory space with no direct access to the filesystem, network, or host process.

```
┌─────────────────────────────────────────────────────────────┐
│                   SoundTime Server (host)                     │
│                                                               │
│  ┌────────────┐       ┌────────────────────────────────┐     │
│  │ API Routes │──────►│        PluginRegistry           │     │
│  └────────────┘       │  ┌────────┐  ┌────────┐        │     │
│         │             │  │Plugin A│  │Plugin B│  ...    │     │
│         │  dispatch   │  │ (WASM) │  │ (WASM) │        │     │
│         ▼             │  └───┬────┘  └───┬────┘        │     │
│  ┌────────────┐       │  ┌───▼──────────▼────────────┐ │     │
│  │Event System│──────►│  │   Extism Runtime          │ │     │
│  └────────────┘       │  │  • 32 MB memory / plugin  │ │     │
│                       │  │  • 1M fuel (instructions)  │ │     │
│                       │  │  • Host functions only     │ │     │
│                       │  └───────────────────────────┘ │     │
│                       └────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────┘
```

Communication between a plugin and the host happens exclusively through **host functions** — a well-defined API that the plugin calls to read tracks, make HTTP requests, log messages, and more.

### Security model

The plugin system follows a **capability-based security** approach:

- **Memory isolation** — Each plugin gets its own WASM linear memory (default 32 MB). It cannot read or write host memory.
- **No filesystem access** — Plugins cannot read or write files. All data access goes through host functions.
- **No direct network access** — HTTP requests are proxied through host functions and restricted to declared hosts.
- **Fuel metering** — Every WASM instruction consumes fuel. When fuel runs out, execution stops. This prevents infinite loops.
- **Crash containment** — A panic or trap inside a plugin cannot crash the SoundTime process.
- **Permission-based** — Plugins declare the permissions they need in `plugin.toml`. The admin reviews and approves them before enabling.

---

## Plugin Manifest (`plugin.toml`)

Every plugin must include a `plugin.toml` file at the root of its repository. This manifest declares metadata, build paths, permissions, and optional UI configuration.

### Full format

```toml
[plugin]
name = "my-plugin"                        # Required — unique identifier
version = "0.1.0"                         # Required — semver
description = "What this plugin does"     # Required — short description
author = "Your Name"                      # Optional
license = "MIT"                           # Optional — SPDX identifier
homepage = "https://github.com/you/repo"  # Optional
min_soundtime_version = "0.1.0"           # Optional — minimum compatible version

[build]
wasm = "target/wasm32-wasi/release/my_plugin.wasm"  # Required — path to WASM binary

[permissions]
http_hosts = ["api.example.com"]                     # Allowed HTTP hosts (default: [])
events = ["on_track_added", "on_user_registered"]    # Events to subscribe to (default: [])
write_tracks = false                                 # Can modify track metadata (default: false)
config_access = true                                 # Can read/write plugin config (default: false)
read_users = false                                   # Can read user information (default: false)

[ui]
enabled = false                    # Whether plugin has a frontend panel (default: false)
slot = "track-detail-sidebar"      # UI slot name (required if ui.enabled = true)
entry = "ui/index.html"            # Entry point HTML file (required if ui.enabled = true)
```

### Validation rules

| Field | Required | Constraints |
|-------|----------|-------------|
| `plugin.name` | Yes | Regex `^[a-z][a-z0-9-]{1,63}$`, must be unique per instance |
| `plugin.version` | Yes | Valid semver (`x.y.z`) |
| `plugin.description` | Yes | 1–500 characters |
| `plugin.author` | No | 1–255 characters if present |
| `plugin.license` | No | Valid SPDX identifier if present |
| `plugin.min_soundtime_version` | No | Valid semver; checked against the running instance version |
| `build.wasm` | Yes | Must exist, `.wasm` extension, size ≤ `PLUGIN_WASM_MAX_SIZE_MB` |
| `permissions.http_hosts` | No | List of domain names (no IPs, no wildcards except `"*"`) |
| `permissions.events` | No | Each value must be a recognized event name |
| `ui.slot` | If `ui.enabled` | Must be a predefined slot name |
| `ui.entry` | If `ui.enabled` | Must exist, `.html` extension |

---

## Available Events

Plugins subscribe to system events by listing them in `permissions.events`. When an event fires, the plugin's corresponding handler function is called with a JSON payload.

### Event reference

| Event | Payload fields | Triggered when |
|-------|---------------|----------------|
| `on_track_added` | `track_id: String`, `title: String`, `artist: String`, `album: Option<String>` | A track is uploaded or discovered via library scan |
| `on_track_played` | `track_id: String`, `user_id: String`, `timestamp: String` | A user starts playing a track |
| `on_track_deleted` | `track_id: String` | A track is deleted |
| `on_library_scan_complete` | `library_id: String`, `tracks_added: u64`, `tracks_removed: u64` | A library scan finishes |
| `on_user_registered` | `user_id: String`, `username: String` | A new user registers |
| `on_user_login` | `user_id: String`, `timestamp: String` | A user logs in |
| `on_playlist_created` | `playlist_id: String`, `user_id: String`, `name: String` | A playlist is created |
| `on_peer_connected` | `peer_id: String`, `domain: Option<String>` | A P2P peer connects |
| `on_peer_disconnected` | `peer_id: String` | A P2P peer disconnects |
| `on_plugin_event` | `source_plugin: String`, `event_type: String`, `data: Value` | Another plugin emits a custom event |

### Writing event handlers

Each handler function must be:

1. **Named** `handle_{event_name}` (e.g., `handle_on_track_added`)
2. **Exported** from the WASM module using the `#[plugin_fn]` attribute
3. **Accept** a `Json<T>` input matching the event payload
4. **Return** `FnResult<Json<()>>`

```rust
use extism_pdk::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct TrackAddedPayload {
    track_id: String,
    title: String,
    artist: String,
    album: Option<String>,
}

#[plugin_fn]
pub fn handle_on_track_added(Json(payload): Json<TrackAddedPayload>) -> FnResult<Json<()>> {
    log_info(&format!("New track: {} by {}", payload.title, payload.artist))?;
    Ok(Json(()))
}
```

### Dispatch behavior

- Events are **fire-and-forget** — return values do not affect the host.
- If a handler panics or exhausts its fuel, the error is logged and dispatch continues to the next subscribed plugin.
- Execution time is recorded in the `plugin_events_log` database table (when `PLUGIN_LOG_EVENTS=true`).
- Plugins receive events in the order they were loaded.

---

## Host Functions

Host functions are the plugin's only interface to the SoundTime server. Each call is permission-checked before execution.

### Track operations

| Function | Signature | Permission | Description |
|----------|-----------|-----------|-------------|
| `get_track` | `(id: String) -> TrackInfo` | None | Get track metadata by ID |
| `search_tracks` | `(query: String, page: u32, per_page: u32) -> PaginatedTracks` | None | Search the local track catalog |
| `set_track_lyrics` | `(track_id: String, lyrics: String)` | `write_tracks` | Set or replace a track's lyrics |
| `set_track_genre` | `(track_id: String, genre: String)` | `write_tracks` | Set or replace a track's genre |
| `set_track_metadata` | `(track_id: String, fields: Map)` | `write_tracks` | Update arbitrary metadata fields |

### User operations

| Function | Signature | Permission | Description |
|----------|-----------|-----------|-------------|
| `list_users` | `(page: u32, per_page: u32) -> Vec<UserInfo>` | `read_users` | List users (paginated) |
| `get_user` | `(id: String) -> UserInfo` | `read_users` | Get a single user's info |

> **Note**: User data never includes password hashes or JWT tokens.

### HTTP operations

| Function | Signature | Permission | Description |
|----------|-----------|-----------|-------------|
| `http_get` | `(url: String, headers: Map) -> HttpResponse` | `http_hosts` | Send a GET request |
| `http_post` | `(url: String, body: String, headers: Map) -> HttpResponse` | `http_hosts` | Send a POST request |

The request URL's host must match one of the domains listed in `permissions.http_hosts`. Requests to unlisted hosts are rejected before any network call is made. Requests time out after `PLUGIN_HTTP_TIMEOUT_SECS` seconds (default: 10).

### Configuration

| Function | Signature | Permission | Description |
|----------|-----------|-----------|-------------|
| `get_config` | `(key: String) -> Option<String>` | `config_access` | Read a config value |
| `set_config` | `(key: String, value: String)` | `config_access` | Write a config value |
| `delete_config` | `(key: String)` | `config_access` | Delete a config entry |

Configuration is a key-value store scoped to the plugin. One plugin cannot access another plugin's configuration. Admins can view and edit config values from the admin panel.

### Logging

| Function | Signature | Permission | Description |
|----------|-----------|-----------|-------------|
| `log_info` | `(message: String)` | None | Log at info level |
| `log_warn` | `(message: String)` | None | Log at warning level |
| `log_error` | `(message: String)` | None | Log at error level |

Log output is routed through the host's `tracing` infrastructure. Messages appear in the server logs prefixed with the plugin name.

### System

| Function | Signature | Permission | Description |
|----------|-----------|-----------|-------------|
| `get_instance_info` | `() -> InstanceInfo` | None | Get the SoundTime instance name, version, domain, and counts |
| `get_current_timestamp` | `() -> String` | None | Get the current UTC time (ISO 8601) |

`get_current_timestamp` exists because WASM modules have no access to the system clock.

### Data types

```rust
/// Track metadata exposed to plugins.
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

/// Paginated track list.
pub struct PaginatedTracks {
    pub tracks: Vec<TrackInfo>,
    pub total: u64,
    pub page: u32,
    pub per_page: u32,
    pub total_pages: u32,
}

/// User information (no sensitive fields).
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub display_name: Option<String>,
    pub role: String,       // "admin" or "user"
    pub created_at: String,
}

/// HTTP response from http_get / http_post.
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

/// SoundTime instance information.
pub struct InstanceInfo {
    pub name: String,
    pub version: String,
    pub domain: String,
    pub track_count: u64,
    pub user_count: u64,
}
```

---

## Building a Plugin

### Prerequisites

- [Rust](https://rustup.rs/) 1.93 or later
- The `wasm32-wasi` target:
  ```bash
  rustup target add wasm32-wasi
  ```

### Step by step

```bash
# 1. Create a new Rust library project
cargo new --lib my-plugin
cd my-plugin

# 2. Add dependencies
cargo add extism-pdk serde serde_json
```

Edit `Cargo.toml` to configure the library type and optimize for size:

```toml
[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = "s"
lto = true
strip = true
```

Write your plugin code in `src/lib.rs` (see [Example Plugins](#example-plugins) below), then build:

```bash
# 3. Build the WASM binary
cargo build --release --target wasm32-wasi
```

The compiled binary will be at `target/wasm32-wasi/release/my_plugin.wasm`.

Create a `plugin.toml` at the project root referencing this path, commit everything to a git repository, and your plugin is ready to install.

---

## Plugin Structure

A typical plugin repository looks like this:

```
my-plugin/
├── plugin.toml                              # Manifest (required)
├── Cargo.toml                               # Rust project config
├── src/
│   └── lib.rs                               # Plugin logic with #[plugin_fn] handlers
├── ui/                                      # Optional: frontend files
│   └── index.html
└── target/
    └── wasm32-wasi/release/my_plugin.wasm   # Compiled WASM binary
```

> **Tip**: Add `target/` to `.gitignore` and include only the compiled `.wasm` file in releases, or build it as part of your CI pipeline and commit the artifact.

---

## UI Slots

Plugins can render custom UI panels in specific locations within the SoundTime frontend. UI is served inside sandboxed iframes (`sandbox="allow-scripts"`, no `allow-same-origin`).

### Available slots

| Slot | Location | Use case |
|------|----------|----------|
| `track-detail-sidebar` | Sidebar on the track detail page | Lyrics, MusicBrainz info, notes |
| `player-extra-controls` | Extra controls area in the audio player | External "like" button, social sharing |
| `library-toolbar` | Toolbar above the library view | Export, external sync, custom filters |
| `settings-panel` | Additional panel in user settings | Plugin-specific preferences |

### Communication via `postMessage`

The iframe communicates with the SoundTime host using the [postMessage API](https://developer.mozilla.org/en-US/docs/Web/API/Window/postMessage):

```javascript
// ─── Plugin iframe → SoundTime ──────────────────────────────

// Send a message to the host application
window.parent.postMessage({
  type: "soundtime:plugin",
  action: "get-current-track",
  data: {}
}, "*");

// ─── SoundTime → Plugin iframe ─────────────────────────────

// Listen for messages from the host
window.addEventListener("message", (event) => {
  if (event.data.type === "soundtime:host") {
    const { action, data } = event.data;
    // Handle host messages (e.g., track changed, theme update)
  }
});
```

### Enabling UI

Set `[ui] enabled = true` in `plugin.toml`, specify the `slot` and `entry` path, and include the HTML file in your repository:

```toml
[ui]
enabled = true
slot = "track-detail-sidebar"
entry = "ui/index.html"
```

The entry HTML file is served by the backend at `/api/admin/plugins/{id}/ui/index.html` and rendered inside the iframe.

---

## Installation & Management

### Installing a plugin

1. Navigate to the **Admin Panel** → **Plugins** tab.
2. Click **Install Plugin** and enter the git repository URL (e.g., `https://github.com/soundtime-plugins/lyrics-fetcher.git`).
3. SoundTime clones the repository, validates the manifest and WASM binary, and registers the plugin with status **disabled**.
4. Review the requested permissions.
5. Click **Enable** to activate the plugin.

Plugins can also be installed via the API:

```bash
curl -X POST http://localhost:8080/api/admin/plugins/install \
  -H "Authorization: Bearer <admin_token>" \
  -H "Content-Type: application/json" \
  -d '{"git_url": "https://github.com/soundtime-plugins/lyrics-fetcher.git"}'
```

### Managing plugins

From the admin panel, administrators can:

- **Enable / Disable** — Toggle a plugin on or off without uninstalling it.
- **Update** — Pull the latest version from the git repository and reload.
- **Uninstall** — Remove the plugin, its configuration, and event logs.
- **Configure** — Edit key-value configuration pairs that the plugin reads via `get_config`.
- **View logs** — Inspect the event execution history (event name, result, execution time, errors).

### Plugin lifecycle

```
Install (git clone) → Disabled → Enable → Running
                                           │
                              Disable ◄────┘
                                           │
                              Update ──────┘ (git pull, revalidate, reload)
                                           │
                              Uninstall ────→ Removed
```

---

## Resource Limits

Every plugin runs under strict resource constraints to protect the host. All limits are configurable via environment variables.

| Resource | Default | Env var | Behavior on exceed |
|----------|---------|---------|-------------------|
| Memory | 32 MB | `PLUGIN_MEMORY_LIMIT_MB` | WASM trap — handler execution aborted |
| Fuel (instructions) | 1,000,000 | `PLUGIN_FUEL_LIMIT` | WASM trap — handler execution aborted |
| WASM binary size | 50 MB | `PLUGIN_WASM_MAX_SIZE_MB` | Installation rejected |
| HTTP timeout | 10 seconds | `PLUGIN_HTTP_TIMEOUT_SECS` | Request cancelled, error returned to plugin |

Additional environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `PLUGIN_ENABLED` | `false` | Master switch — set to `true` to enable the plugin system |
| `PLUGIN_DIR` | `/data/plugins` | Directory where installed plugins are stored |
| `PLUGIN_LOG_EVENTS` | `true` | Log every event execution to `plugin_events_log` |

---

## Example Plugins

Three reference plugins demonstrate common patterns. Each is a standalone git repository.

### Lyrics Fetcher

Automatically fetches lyrics from [lyrics.ovh](https://lyrics.ovh) when a new track is added to the library.

**Permissions**: `http_hosts: ["api.lyrics.ovh"]`, `events: ["on_track_added"]`, `write_tracks: true`

**`plugin.toml`**:

```toml
[plugin]
name = "lyrics-fetcher"
version = "0.1.0"
description = "Fetches lyrics from lyrics.ovh when tracks are added"
author = "SoundTime Community"
license = "MIT"

[build]
wasm = "target/wasm32-wasi/release/lyrics_fetcher.wasm"

[permissions]
http_hosts = ["api.lyrics.ovh"]
events = ["on_track_added"]
write_tracks = true
config_access = false
read_users = false

[ui]
enabled = false
```

**`src/lib.rs`**:

```rust
use extism_pdk::*;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct TrackAddedPayload {
    track_id: String,
    title: String,
    artist: String,
    album: Option<String>,
}

#[derive(Deserialize)]
struct LyricsResponse {
    lyrics: String,
}

#[plugin_fn]
pub fn handle_on_track_added(Json(payload): Json<TrackAddedPayload>) -> FnResult<Json<()>> {
    let url = format!(
        "https://api.lyrics.ovh/v1/{}/{}",
        payload.artist, payload.title
    );

    // http_get is a host function provided by SoundTime
    let response: HttpResponse = http_get(&url, &HashMap::new())?;

    if response.status == 200 {
        if let Ok(lyrics_resp) = serde_json::from_str::<LyricsResponse>(&response.body) {
            set_track_lyrics(&payload.track_id, &lyrics_resp.lyrics)?;
            log_info(&format!(
                "Lyrics found for: {} - {}",
                payload.artist, payload.title
            ))?;
        }
    }

    Ok(Json(()))
}
```

### ListenBrainz Scrobbler

Submits listening activity to [ListenBrainz](https://listenbrainz.org/) every time a track is played. The API token is stored in plugin configuration.

**Permissions**: `http_hosts: ["api.listenbrainz.org"]`, `events: ["on_track_played"]`, `config_access: true`

**Key behavior**:
- Reads the `api_token` config value (set by the admin in the plugin configuration panel).
- Calls `get_track` to fetch full metadata for the played track.
- POSTs a scrobble payload to the ListenBrainz API.
- Logs a warning if the API token is not configured.

### Webhook Notifier

Relays SoundTime events to a configurable webhook URL. Useful for sending notifications to Slack, Discord, or custom monitoring systems.

**Permissions**: `http_hosts: ["*"]` (admin must approve), `events: ["on_track_added", "on_user_registered", "on_peer_connected"]`, `config_access: true`

**Key behavior**:
- Reads the `webhook_url` config value.
- Wraps each event in a standard envelope with `event`, `instance`, `timestamp`, and `data` fields.
- Sends the payload as a JSON POST to the configured URL.
- Handles multiple events by implementing a `handle_on_*` function for each subscribed event, all delegating to a shared `send_webhook` helper.

---

## Tips & Best Practices

- **Keep WASM binaries small** — Use `opt-level = "s"`, `lto = true`, and `strip = true` in your release profile. Smaller binaries load faster and use less memory.
- **Handle missing config gracefully** — Always check for `None` when calling `get_config`. Log a warning and return early rather than panicking.
- **Use structured logging** — Include context (track ID, user ID) in log messages to make debugging easier in the admin logs panel.
- **Request minimal permissions** — Only ask for the permissions your plugin actually needs. Admins are more likely to trust and enable plugins with a narrow permission scope.
- **Test locally** — Build your WASM binary, install it on a local SoundTime instance, and verify event handling before publishing.
- **Version your plugin** — Follow [semver](https://semver.org/). Bump the version in `plugin.toml` when publishing updates so admins can track changes.

## Licensing

SoundTime itself is licensed under [AGPL-3.0](https://www.gnu.org/licenses/agpl-3.0.html). Plugins are separate works and can use any license. If your plugin links to or is derived from SoundTime code, consult the AGPL-3.0 terms for obligations. Standalone WASM modules that communicate only through the host function API are generally considered independent works.
