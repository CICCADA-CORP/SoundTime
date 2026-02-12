//! SoundTime Plugin System
//!
//! WASM-based plugin runtime using Extism (wasmtime) for sandboxed execution.
//! Plugins are installed from git repos, run in isolated WASM sandboxes with
//! memory limits and fuel-based execution limits, and communicate with the
//! host through well-defined host functions.

pub mod error;
pub mod events;
pub mod host_functions;
pub mod installer;
pub mod manifest;
pub mod registry;
pub mod sandbox;

pub use error::PluginError;
pub use events::{
    LibraryScanCompletePayload, PeerConnectedPayload, PeerDisconnectedPayload,
    PlaylistCreatedPayload, PluginEvent, PluginEventPayload, TrackAddedPayload,
    TrackDeletedPayload, TrackPlayedPayload, UserLoginPayload, UserRegisteredPayload, KNOWN_EVENTS,
};
pub use host_functions::HostContext;
pub use installer::PluginInstaller;
pub use manifest::PluginManifest;
pub use registry::PluginRegistry;
pub use sandbox::{PluginSandbox, SandboxConfig};
