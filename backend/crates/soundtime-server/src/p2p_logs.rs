//! P2P log capture — ring-buffer tracing layer that captures soundtime_p2p events.
//!
//! Provides a global `P2pLogBuffer` that stores the last N log entries from the
//! `soundtime_p2p` crate. The admin API can read these entries to display
//! P2P network logs in the dashboard.

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

/// Default ring buffer capacity (how many log entries to keep).
const DEFAULT_CAPACITY: usize = 500;

/// Global P2P log buffer — accessible from both the tracing layer and the API.
pub static P2P_LOG_BUFFER: LazyLock<P2pLogBuffer> =
    LazyLock::new(|| P2pLogBuffer::new(DEFAULT_CAPACITY));

/// A single captured log entry.
#[derive(Debug, Clone, Serialize)]
pub struct P2pLogEntry {
    /// ISO-8601 timestamp
    pub timestamp: String,
    /// Log level: "TRACE", "DEBUG", "INFO", "WARN", "ERROR"
    pub level: String,
    /// Target module path (e.g. "soundtime_p2p::node")
    pub target: String,
    /// The log message
    pub message: String,
    /// Optional structured fields as key=value pairs
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
}

/// Thread-safe ring buffer for P2P log entries.
pub struct P2pLogBuffer {
    entries: Mutex<VecDeque<P2pLogEntry>>,
    capacity: usize,
}

impl P2pLogBuffer {
    /// Create a new buffer with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    /// Push a new entry, evicting the oldest if at capacity.
    pub fn push(&self, entry: P2pLogEntry) {
        let mut entries = self.entries.lock().unwrap();
        if entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    /// Return all entries (oldest first). Optionally filter by level and limit count.
    pub fn entries(&self, level_filter: Option<&str>, limit: Option<usize>) -> Vec<P2pLogEntry> {
        let entries = self.entries.lock().unwrap();
        let iter = entries.iter().filter(|e| {
            if let Some(lvl) = level_filter {
                e.level.eq_ignore_ascii_case(lvl)
            } else {
                true
            }
        });
        match limit {
            Some(n) => iter
                .rev()
                .take(n)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
            None => iter.cloned().collect(),
        }
    }

    /// Clear all entries.
    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }

    /// Return the number of entries currently stored.
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

/// Visitor that extracts the message and structured fields from a tracing event.
struct FieldCollector {
    message: String,
    fields: Vec<String>,
}

impl FieldCollector {
    fn new() -> Self {
        Self {
            message: String::new(),
            fields: Vec::new(),
        }
    }
}

impl Visit for FieldCollector {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
            // Strip surrounding quotes if present
            if self.message.starts_with('"') && self.message.ends_with('"') {
                self.message = self.message[1..self.message.len() - 1].to_string();
            }
        } else {
            self.fields.push(format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={}", field.name(), value));
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.push(format!("{}={}", field.name(), value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.push(format!("{}={}", field.name(), value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.push(format!("{}={}", field.name(), value));
    }
}

/// Tracing layer that captures events from `soundtime_p2p` and related modules.
pub struct P2pLogLayer {
    buffer: &'static P2pLogBuffer,
}

impl P2pLogLayer {
    /// Create a new layer backed by the global `P2P_LOG_BUFFER`.
    pub fn new() -> Self {
        Self {
            buffer: &P2P_LOG_BUFFER,
        }
    }
}

/// List of target prefixes to capture. Includes soundtime_p2p and iroh internals.
const CAPTURED_TARGETS: &[&str] = &["soundtime_p2p", "iroh", "iroh_blobs", "iroh_relay"];

/// Returns true if a tracing target should be captured.
fn should_capture(target: &str) -> bool {
    CAPTURED_TARGETS
        .iter()
        .any(|prefix| target.starts_with(prefix))
}

impl<S> Layer<S> for P2pLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let target = metadata.target();

        // Only capture P2P-related events
        if !should_capture(target) {
            return;
        }

        // Skip TRACE level to avoid excessive noise (unless iroh which rarely traces)
        if *metadata.level() == Level::TRACE && target.starts_with("soundtime_p2p") {
            return;
        }

        let mut collector = FieldCollector::new();
        event.record(&mut collector);

        let entry = P2pLogEntry {
            timestamp: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            level: metadata.level().to_string(),
            target: target.to_string(),
            message: collector.message,
            fields: collector.fields,
        };

        self.buffer.push(entry);
    }
}

// ── API handler ─────────────────────────────────────────────────

use axum::extract::Query;
use axum::Json;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct LogQuery {
    /// Filter by log level (e.g. "ERROR", "WARN", "INFO", "DEBUG")
    pub level: Option<String>,
    /// Maximum number of entries to return (default: all)
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct LogResponse {
    pub entries: Vec<P2pLogEntry>,
    pub total_in_buffer: usize,
}

/// GET /api/admin/p2p/logs — retrieve captured P2P log entries.
pub async fn get_p2p_logs(Query(params): Query<LogQuery>) -> Json<LogResponse> {
    let entries = P2P_LOG_BUFFER.entries(params.level.as_deref(), params.limit);
    let total_in_buffer = P2P_LOG_BUFFER.len();
    Json(LogResponse {
        entries,
        total_in_buffer,
    })
}

/// DELETE /api/admin/p2p/logs — clear the P2P log buffer.
pub async fn clear_p2p_logs() -> Json<serde_json::Value> {
    P2P_LOG_BUFFER.clear();
    Json(serde_json::json!({ "status": "cleared" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_evicts_oldest() {
        let buf = P2pLogBuffer::new(3);
        for i in 0..5 {
            buf.push(P2pLogEntry {
                timestamp: format!("t{i}"),
                level: "INFO".to_string(),
                target: "test".to_string(),
                message: format!("msg {i}"),
                fields: vec![],
            });
        }
        let entries = buf.entries(None, None);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].message, "msg 2");
        assert_eq!(entries[2].message, "msg 4");
    }

    #[test]
    fn filter_by_level() {
        let buf = P2pLogBuffer::new(10);
        buf.push(P2pLogEntry {
            timestamp: "t1".to_string(),
            level: "INFO".to_string(),
            target: "test".to_string(),
            message: "info msg".to_string(),
            fields: vec![],
        });
        buf.push(P2pLogEntry {
            timestamp: "t2".to_string(),
            level: "ERROR".to_string(),
            target: "test".to_string(),
            message: "error msg".to_string(),
            fields: vec![],
        });
        let errors = buf.entries(Some("ERROR"), None);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "error msg");
    }

    #[test]
    fn limit_entries() {
        let buf = P2pLogBuffer::new(10);
        for i in 0..10 {
            buf.push(P2pLogEntry {
                timestamp: format!("t{i}"),
                level: "INFO".to_string(),
                target: "test".to_string(),
                message: format!("msg {i}"),
                fields: vec![],
            });
        }
        let entries = buf.entries(None, Some(3));
        assert_eq!(entries.len(), 3);
        // Should return the LAST 3 entries
        assert_eq!(entries[0].message, "msg 7");
        assert_eq!(entries[2].message, "msg 9");
    }

    #[test]
    fn should_capture_targets() {
        assert!(should_capture("soundtime_p2p::node"));
        assert!(should_capture("soundtime_p2p::discovery"));
        assert!(should_capture("iroh::socket::transports"));
        assert!(should_capture("iroh_blobs::store"));
        assert!(!should_capture("soundtime_server::api"));
        assert!(!should_capture("sea_orm"));
    }
}
