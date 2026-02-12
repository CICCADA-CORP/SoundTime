//! Plugin event system — defines events and dispatch logic.

use serde::{Deserialize, Serialize};

/// Known event names that plugins can subscribe to.
pub const KNOWN_EVENTS: &[&str] = &[
    "on_track_added",
    "on_track_played",
    "on_track_deleted",
    "on_library_scan_complete",
    "on_user_registered",
    "on_user_login",
    "on_playlist_created",
    "on_peer_connected",
    "on_peer_disconnected",
    "on_plugin_event",
];

/// A plugin event with its payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEvent {
    pub name: String,
    pub payload: serde_json::Value,
}

impl PluginEvent {
    /// Create a new event.
    pub fn new(name: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            payload,
        }
    }

    /// Check if this event name is a known system event.
    pub fn is_known_event(name: &str) -> bool {
        KNOWN_EVENTS.contains(&name)
    }
}

// ─── Event payload types ─────────────────────────────────────────────

/// Payload for `on_track_added` events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackAddedPayload {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
}

/// Payload for `on_track_played` events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackPlayedPayload {
    pub track_id: String,
    pub user_id: String,
    pub timestamp: String,
}

/// Payload for `on_track_deleted` events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackDeletedPayload {
    pub track_id: String,
}

/// Payload for `on_library_scan_complete` events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryScanCompletePayload {
    pub library_id: String,
    pub tracks_added: u64,
    pub tracks_removed: u64,
}

/// Payload for `on_user_registered` events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRegisteredPayload {
    pub user_id: String,
    pub username: String,
}

/// Payload for `on_user_login` events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserLoginPayload {
    pub user_id: String,
    pub timestamp: String,
}

/// Payload for `on_playlist_created` events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistCreatedPayload {
    pub playlist_id: String,
    pub user_id: String,
    pub name: String,
}

/// Payload for `on_peer_connected` events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConnectedPayload {
    pub peer_id: String,
    pub domain: Option<String>,
    pub track_count: Option<u64>,
}

/// Payload for `on_peer_disconnected` events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDisconnectedPayload {
    pub peer_id: String,
}

/// Payload for `on_plugin_event` (inter-plugin communication).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEventPayload {
    pub source_plugin: String,
    pub event_name: String,
    pub payload: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_events_contains_expected() {
        assert!(PluginEvent::is_known_event("on_track_added"));
        assert!(PluginEvent::is_known_event("on_track_played"));
        assert!(PluginEvent::is_known_event("on_peer_connected"));
        assert!(!PluginEvent::is_known_event("on_unknown_event"));
    }

    #[test]
    fn test_event_creation() {
        let event = PluginEvent::new(
            "on_track_added",
            serde_json::json!({
                "track_id": "abc123",
                "title": "Test Track",
            }),
        );
        assert_eq!(event.name, "on_track_added");
        assert_eq!(event.payload["track_id"], "abc123");
    }

    #[test]
    fn test_known_events_count() {
        assert_eq!(KNOWN_EVENTS.len(), 10);
    }

    #[test]
    fn test_track_added_payload_serialization() {
        let payload = TrackAddedPayload {
            track_id: "abc".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: Some("Album".into()),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["track_id"], "abc");
        assert_eq!(json["title"], "Song");
    }

    #[test]
    fn test_track_played_payload_serialization() {
        let payload = TrackPlayedPayload {
            track_id: "abc".into(),
            user_id: "user-1".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["user_id"], "user-1");
    }

    #[test]
    fn test_all_payload_types_serialize() {
        // Verify all payload types can round-trip through JSON
        let _ = serde_json::to_value(TrackDeletedPayload {
            track_id: "x".into(),
        })
        .unwrap();
        let _ = serde_json::to_value(LibraryScanCompletePayload {
            library_id: "lib".into(),
            tracks_added: 10,
            tracks_removed: 2,
        })
        .unwrap();
        let _ = serde_json::to_value(UserRegisteredPayload {
            user_id: "u".into(),
            username: "test".into(),
        })
        .unwrap();
        let _ = serde_json::to_value(UserLoginPayload {
            user_id: "u".into(),
            timestamp: "now".into(),
        })
        .unwrap();
        let _ = serde_json::to_value(PlaylistCreatedPayload {
            playlist_id: "pl".into(),
            user_id: "u".into(),
            name: "My Playlist".into(),
        })
        .unwrap();
        let _ = serde_json::to_value(PeerConnectedPayload {
            peer_id: "p".into(),
            domain: Some("example.com".into()),
            track_count: Some(100),
        })
        .unwrap();
        let _ = serde_json::to_value(PeerDisconnectedPayload {
            peer_id: "p".into(),
        })
        .unwrap();
        let _ = serde_json::to_value(PluginEventPayload {
            source_plugin: "src".into(),
            event_name: "custom".into(),
            payload: serde_json::json!({}),
        })
        .unwrap();
    }
}
