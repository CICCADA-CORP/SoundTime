pub mod admin;
pub mod albums;
pub mod artists;
pub mod audio;
pub mod editorial;
pub mod favorites;
pub mod history;
pub mod libraries;
pub mod lyrics;
pub mod p2p;
pub mod playlists;
pub mod plugins;
pub mod reports;
pub mod search;
pub mod setup;
pub mod stats;
pub mod themes;
pub mod tracks;
pub mod users;

use soundtime_db::AppState;
use std::sync::Arc;

/// Extract the plugin registry from type-erased application state.
///
/// Returns `None` if the plugin system is not enabled (`PLUGIN_ENABLED=false`).
pub fn get_plugin_registry(state: &AppState) -> Option<Arc<soundtime_plugin::PluginRegistry>> {
    state.plugins.as_ref().and_then(|any| {
        any.clone()
            .downcast::<soundtime_plugin::PluginRegistry>()
            .ok()
    })
}
