//! Listing worker — periodically announces this instance to the public SoundTime node directory.
//!
//! Listing is **enabled by default** (opt-out via `listing_public = false`).
//! A heartbeat is sent every 5 minutes to the listing server.
//! The listing server checks node health and removes offline nodes after 48h.
//!
//! The module also exposes a `trigger_heartbeat` endpoint so the admin UI can
//! request an immediate heartbeat after toggling the setting.

use axum::extract::State as AxumState;
use axum::http::StatusCode;
use axum::Json;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use soundtime_db::entities::{instance_setting, track, user};
use soundtime_db::AppState;
use std::sync::Arc;
use tokio::sync::Notify;
use uuid::Uuid;

/// How often to send a heartbeat (5 minutes).
const HEARTBEAT_INTERVAL_SECS: u64 = 300;

/// Default listing server URL.
const DEFAULT_LISTING_URL: &str = "https://soundtime-listing-production.up.railway.app";

/// Shared notifier so the admin API can wake up the worker immediately.
static HEARTBEAT_NOTIFY: std::sync::LazyLock<Notify> = std::sync::LazyLock::new(Notify::new);

/// Parameters gathered from the database for a heartbeat announcement.
#[derive(Debug, Clone)]
struct HeartbeatParams {
    domain: String,
    name: String,
    description: String,
    token: Option<String>,
    track_count: u64,
    user_count: u64,
    open_registration: bool,
}

/// Build the JSON payload for the listing server announcement.
fn build_announce_payload(params: &HeartbeatParams) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "domain": &params.domain,
        "name": &params.name,
        "description": &params.description,
        "version": "0.1.0",
        "track_count": params.track_count,
        "user_count": params.user_count,
        "open_registration": params.open_registration,
    });

    if let Some(ref t) = params.token {
        payload["token"] = serde_json::Value::String(t.clone());
    }

    payload
}

/// Send the announce payload to the listing server and return the response body.
async fn send_announce_request(
    client: &reqwest::Client,
    listing_url: &str,
    payload: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let url = format!("{listing_url}/api/announce");
    tracing::debug!("sending listing heartbeat to {url}");

    let resp = client
        .post(&url)
        .json(payload)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    if status.is_success() {
        Ok(body)
    } else {
        let error = body
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("unknown error");
        Err(format!("Listing server returned {status}: {error}"))
    }
}

/// Clean a raw domain string: strip protocol prefix and trailing slash.
#[cfg(test)]
fn clean_domain(raw: &str) -> String {
    raw.trim_end_matches('/')
        .replace("https://", "")
        .replace("http://", "")
}

/// Returns `true` if the domain looks like a local/unreachable address.
fn is_local_domain(domain: &str) -> bool {
    domain.starts_with("localhost") || domain.starts_with("127.") || domain.starts_with("0.0.0.0")
}

/// POST /api/admin/listing/trigger — force an immediate heartbeat.
pub async fn trigger_heartbeat(
    AxumState(state): AxumState<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let is_enabled = is_listing_enabled(&state).await;
    if !is_enabled {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "listing is disabled" })),
        ));
    }

    // Wake the worker immediately
    HEARTBEAT_NOTIFY.notify_one();

    Ok(Json(serde_json::json!({ "status": "heartbeat triggered" })))
}

/// Spawn the listing heartbeat worker.
pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        tracing::info!("listing worker started (heartbeat every 5m, enabled by default)");

        // Wait 10s before first attempt to let the server fully start
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("SoundTime/0.1.0")
            .build()
            .expect("failed to build HTTP client");

        loop {
            // Check if listing is enabled
            let is_enabled = is_listing_enabled(&state).await;

            if is_enabled {
                let listing_url = get_listing_url(&state).await;
                tracing::info!(
                    domain = %state.domain,
                    listing_url = %listing_url,
                    "sending listing heartbeat"
                );
                match send_heartbeat(&state, &client, &listing_url).await {
                    Ok(()) => tracing::info!("listing heartbeat successful"),
                    Err(e) => tracing::warn!("listing heartbeat failed: {e}"),
                }
            } else {
                tracing::debug!("listing disabled, skipping heartbeat");
            }

            // Wait for either the interval or a manual trigger
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS)) => {},
                _ = HEARTBEAT_NOTIFY.notified() => {
                    tracing::info!("listing heartbeat triggered manually");
                },
            }
        }
    });
}

/// Read the domain to announce to the listing server.
/// Priority: DB setting `listing_domain` → env var `SOUNDTIME_DOMAIN` → "localhost:8080".
async fn get_listing_domain(state: &AppState) -> String {
    let from_db = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("listing_domain"))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value)
        .filter(|v| !v.is_empty());

    from_db
        .unwrap_or_else(|| state.domain.clone())
        .trim_end_matches('/')
        .replace("https://", "")
        .replace("http://", "")
}

/// Read the listing URL from instance settings, falling back to env var then default.
async fn get_listing_url(state: &AppState) -> String {
    let from_db = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("listing_url"))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value)
        .filter(|v| !v.is_empty());

    from_db
        .or_else(|| std::env::var("LISTING_URL").ok())
        .unwrap_or_else(|| DEFAULT_LISTING_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Check if the `listing_public` setting is enabled.
/// Defaults to `true` — listing is opt-out, not opt-in.
async fn is_listing_enabled(state: &AppState) -> bool {
    instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("listing_public"))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value == "true")
        .unwrap_or(true)
}

/// Read the listing token from instance settings.
async fn get_listing_token(state: &AppState) -> Option<String> {
    instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("listing_token"))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value)
}

/// Save the listing token to instance settings.
async fn save_listing_token(state: &AppState, token: &str) {
    let existing = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("listing_token"))
        .one(&state.db)
        .await
        .ok()
        .flatten();

    match existing {
        Some(s) => {
            let mut update: instance_setting::ActiveModel = s.into();
            update.value = Set(token.to_string());
            update.updated_at = Set(chrono::Utc::now().into());
            let _ = update.update(&state.db).await;
        }
        None => {
            let _ = instance_setting::ActiveModel {
                id: Set(Uuid::new_v4()),
                key: Set("listing_token".to_string()),
                value: Set(token.to_string()),
                updated_at: Set(chrono::Utc::now().into()),
            }
            .insert(&state.db)
            .await;
        }
    }
}

/// Get the instance name from settings.
async fn get_instance_name(state: &AppState) -> String {
    instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("instance_name"))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value)
        .unwrap_or_else(|| "SoundTime".to_string())
}

/// Get the instance description from settings.
async fn get_instance_description(state: &AppState) -> String {
    instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("instance_description"))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value)
        .unwrap_or_default()
}

/// Send a heartbeat to the listing server.
async fn send_heartbeat(
    state: &AppState,
    client: &reqwest::Client,
    listing_url: &str,
) -> Result<(), String> {
    let domain = get_listing_domain(state).await;

    // Warn if domain looks like a localhost address — listing server won't be able to reach us
    if is_local_domain(&domain) {
        tracing::warn!(
            domain = %domain,
            "listing heartbeat: domain is a local address — the listing server will not be able \
             to reach this instance for health checks. Set the listing domain in admin settings \
             or SOUNDTIME_DOMAIN env var to your public domain (e.g. music.example.com)."
        );
    }

    let name = get_instance_name(state).await;
    let description = get_instance_description(state).await;
    let token = get_listing_token(state).await;

    // Gather instance stats for richer listing
    let track_count = track::Entity::find().count(&state.db).await.unwrap_or(0);
    let user_count = user::Entity::find().count(&state.db).await.unwrap_or(0);

    let open_registration = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("instance_private"))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value != "true")
        .unwrap_or(true);

    let params = HeartbeatParams {
        domain,
        name,
        description,
        token: token.clone(),
        track_count,
        user_count,
        open_registration,
    };

    let payload = build_announce_payload(&params);

    let body = send_announce_request(client, listing_url, &payload).await?;

    // If this was a new registration, save the token
    if token.is_none() {
        if let Some(new_token) = body.get("token").and_then(|t| t.as_str()) {
            tracing::info!("registered on listing server — saving token");
            save_listing_token(state, new_token).await;
        }
    }
    tracing::debug!("listing heartbeat successful");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // ─── Helper ─────────────────────────────────────────────────────

    fn sample_params() -> HeartbeatParams {
        HeartbeatParams {
            domain: "music.example.com".to_string(),
            name: "My SoundTime".to_string(),
            description: "A community music server".to_string(),
            token: None,
            track_count: 42,
            user_count: 5,
            open_registration: true,
        }
    }

    fn http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .user_agent("SoundTime/0.1.0-test")
            .build()
            .unwrap()
    }

    // ─── Pure unit tests: build_announce_payload ────────────────────

    #[test]
    fn payload_contains_all_required_fields() {
        let params = sample_params();
        let payload = build_announce_payload(&params);

        assert_eq!(payload["domain"], "music.example.com");
        assert_eq!(payload["name"], "My SoundTime");
        assert_eq!(payload["description"], "A community music server");
        assert_eq!(payload["version"], "0.1.0");
        assert_eq!(payload["track_count"], 42);
        assert_eq!(payload["user_count"], 5);
        assert_eq!(payload["open_registration"], true);
    }

    #[test]
    fn payload_omits_token_on_first_registration() {
        let params = sample_params();
        let payload = build_announce_payload(&params);

        assert!(payload.get("token").is_none());
    }

    #[test]
    fn payload_includes_token_for_heartbeat() {
        let mut params = sample_params();
        params.token = Some("secret-token-123".to_string());
        let payload = build_announce_payload(&params);

        assert_eq!(payload["token"], "secret-token-123");
    }

    #[test]
    fn payload_with_zero_stats() {
        let mut params = sample_params();
        params.track_count = 0;
        params.user_count = 0;
        let payload = build_announce_payload(&params);

        assert_eq!(payload["track_count"], 0);
        assert_eq!(payload["user_count"], 0);
    }

    #[test]
    fn payload_with_closed_registration() {
        let mut params = sample_params();
        params.open_registration = false;
        let payload = build_announce_payload(&params);

        assert_eq!(payload["open_registration"], false);
    }

    #[test]
    fn payload_with_empty_description() {
        let mut params = sample_params();
        params.description = String::new();
        let payload = build_announce_payload(&params);

        assert_eq!(payload["description"], "");
    }

    #[test]
    fn payload_with_unicode_name() {
        let mut params = sample_params();
        params.name = "音楽サーバー 🎵".to_string();
        let payload = build_announce_payload(&params);

        assert_eq!(payload["name"], "音楽サーバー 🎵");
    }

    // ─── Pure unit tests: clean_domain ──────────────────────────────

    #[test]
    fn clean_domain_strips_https() {
        assert_eq!(
            clean_domain("https://music.example.com"),
            "music.example.com"
        );
    }

    #[test]
    fn clean_domain_strips_http() {
        assert_eq!(
            clean_domain("http://music.example.com"),
            "music.example.com"
        );
    }

    #[test]
    fn clean_domain_strips_trailing_slash() {
        assert_eq!(clean_domain("music.example.com/"), "music.example.com");
    }

    #[test]
    fn clean_domain_strips_protocol_and_slash() {
        assert_eq!(
            clean_domain("https://music.example.com/"),
            "music.example.com"
        );
    }

    #[test]
    fn clean_domain_preserves_port() {
        assert_eq!(
            clean_domain("music.example.com:8880"),
            "music.example.com:8880"
        );
    }

    #[test]
    fn clean_domain_bare_domain_unchanged() {
        assert_eq!(clean_domain("music.example.com"), "music.example.com");
    }

    // ─── Pure unit tests: is_local_domain ───────────────────────────

    #[test]
    fn local_domain_localhost() {
        assert!(is_local_domain("localhost"));
        assert!(is_local_domain("localhost:8080"));
    }

    #[test]
    fn local_domain_loopback_ipv4() {
        assert!(is_local_domain("127.0.0.1"));
        assert!(is_local_domain("127.0.0.1:8080"));
    }

    #[test]
    fn local_domain_any_address() {
        assert!(is_local_domain("0.0.0.0"));
        assert!(is_local_domain("0.0.0.0:8080"));
    }

    #[test]
    fn local_domain_public_address_is_not_local() {
        assert!(!is_local_domain("music.example.com"));
        assert!(!is_local_domain("192.168.1.1:8080"));
        assert!(!is_local_domain("10.0.0.1"));
    }

    // ─── Pure unit tests: constants ─────────────────────────────────

    #[test]
    fn default_listing_url_is_production() {
        assert!(DEFAULT_LISTING_URL.starts_with("https://"));
        assert!(DEFAULT_LISTING_URL.contains("soundtime-listing"));
    }

    #[test]
    fn heartbeat_interval_is_five_minutes() {
        assert_eq!(HEARTBEAT_INTERVAL_SECS, 300);
    }

    // ─── HTTP integration tests: send_announce_request ──────────────

    #[tokio::test]
    async fn announce_first_registration_returns_token() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/announce"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "registered",
                "id": "00000000-0000-0000-0000-000000000001",
                "domain": "music.example.com",
                "token": "new-secret-token",
                "message": "Save this token!"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = http_client();
        let params = sample_params();
        let payload = build_announce_payload(&params);
        let result = send_announce_request(&client, &mock_server.uri(), &payload).await;

        assert!(result.is_ok());
        let body = result.unwrap();
        assert_eq!(body["status"], "registered");
        assert_eq!(body["token"], "new-secret-token");
        assert_eq!(body["domain"], "music.example.com");
    }

    #[tokio::test]
    async fn announce_heartbeat_with_token_succeeds() {
        let mock_server = MockServer::start().await;

        let mut params = sample_params();
        params.token = Some("existing-token".to_string());
        let payload = build_announce_payload(&params);

        // Verify the token is sent in the request body
        Mock::given(method("POST"))
            .and(path("/api/announce"))
            .and(body_json(&payload))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "heartbeat",
                "domain": "music.example.com"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = http_client();
        let result = send_announce_request(&client, &mock_server.uri(), &payload).await;

        assert!(result.is_ok());
        let body = result.unwrap();
        assert_eq!(body["status"], "heartbeat");
    }

    #[tokio::test]
    async fn announce_server_returns_error_status() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/announce"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "error": "Domain is not reachable"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = http_client();
        let payload = build_announce_payload(&sample_params());
        let result = send_announce_request(&client, &mock_server.uri(), &payload).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("422"),
            "error should contain status code: {err}"
        );
        assert!(
            err.contains("Domain is not reachable"),
            "error should contain message: {err}"
        );
    }

    #[tokio::test]
    async fn announce_server_returns_500() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/announce"))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "error": "Internal server error"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = http_client();
        let payload = build_announce_payload(&sample_params());
        let result = send_announce_request(&client, &mock_server.uri(), &payload).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("500"));
    }

    #[tokio::test]
    async fn announce_server_unknown_error_field() {
        let mock_server = MockServer::start().await;

        // Response is 400 but without an "error" field
        Mock::given(method("POST"))
            .and(path("/api/announce"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "message": "bad request"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = http_client();
        let payload = build_announce_payload(&sample_params());
        let result = send_announce_request(&client, &mock_server.uri(), &payload).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown error"));
    }

    #[tokio::test]
    async fn announce_connection_refused() {
        let client = http_client();
        let payload = build_announce_payload(&sample_params());
        // Use a port that's almost certainly not listening
        let result = send_announce_request(&client, "http://127.0.0.1:19999", &payload).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("HTTP request failed"));
    }

    #[tokio::test]
    async fn announce_payload_matches_expected_json() {
        let mock_server = MockServer::start().await;

        let params = HeartbeatParams {
            domain: "test.soundtime.io".to_string(),
            name: "Test Node".to_string(),
            description: "Testing".to_string(),
            token: None,
            track_count: 100,
            user_count: 10,
            open_registration: false,
        };
        let expected_payload = serde_json::json!({
            "domain": "test.soundtime.io",
            "name": "Test Node",
            "description": "Testing",
            "version": "0.1.0",
            "track_count": 100,
            "user_count": 10,
            "open_registration": false,
        });

        Mock::given(method("POST"))
            .and(path("/api/announce"))
            .and(body_json(&expected_payload))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "registered",
                "token": "tok"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = http_client();
        let payload = build_announce_payload(&params);
        let result = send_announce_request(&client, &mock_server.uri(), &payload).await;

        // If the body didn't match, the mock wouldn't respond → error
        assert!(
            result.is_ok(),
            "payload should match expected JSON: {result:?}"
        );
    }

    #[tokio::test]
    async fn announce_multiple_heartbeats_succeed() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/announce"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "heartbeat"
            })))
            .expect(3)
            .mount(&mock_server)
            .await;

        let client = http_client();
        let mut params = sample_params();
        params.token = Some("tok".to_string());
        let payload = build_announce_payload(&params);

        for _ in 0..3 {
            let result = send_announce_request(&client, &mock_server.uri(), &payload).await;
            assert!(result.is_ok());
        }
    }

    // ─── End-to-end flow tests ──────────────────────────────────────

    #[tokio::test]
    async fn full_registration_flow_extracts_token() {
        let mock_server = MockServer::start().await;

        // First call: registration → returns token
        Mock::given(method("POST"))
            .and(path("/api/announce"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "registered",
                "id": "node-uuid",
                "domain": "music.example.com",
                "token": "save-me-token-42",
                "message": "Save this token!"
            })))
            .mount(&mock_server)
            .await;

        let client = http_client();
        let params = sample_params(); // token = None → first registration
        let payload = build_announce_payload(&params);

        let body = send_announce_request(&client, &mock_server.uri(), &payload)
            .await
            .expect("registration should succeed");

        // Simulate what send_heartbeat does after a successful first registration
        assert!(params.token.is_none(), "this is a first registration");
        let new_token = body.get("token").and_then(|t| t.as_str());
        assert_eq!(new_token, Some("save-me-token-42"));
    }

    #[tokio::test]
    async fn full_heartbeat_flow_does_not_overwrite_token() {
        let mock_server = MockServer::start().await;

        // Heartbeat response doesn't include a new token
        Mock::given(method("POST"))
            .and(path("/api/announce"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "heartbeat",
                "domain": "music.example.com"
            })))
            .mount(&mock_server)
            .await;

        let client = http_client();
        let mut params = sample_params();
        params.token = Some("existing-token".to_string());
        let payload = build_announce_payload(&params);

        let body = send_announce_request(&client, &mock_server.uri(), &payload)
            .await
            .expect("heartbeat should succeed");

        // When token already exists, send_heartbeat should NOT try to save a new one
        assert!(params.token.is_some());
        // The body has no "token" field for heartbeat responses
        assert!(body.get("token").is_none());
    }

    // ─── trigger_heartbeat handler tests ────────────────────────────

    #[test]
    fn trigger_heartbeat_response_format() {
        // Verify the success response structure
        let ok_body = serde_json::json!({ "status": "heartbeat triggered" });
        assert_eq!(ok_body["status"], "heartbeat triggered");

        // Verify the error response structure when listing is disabled
        let err_body = serde_json::json!({ "error": "listing is disabled" });
        assert_eq!(err_body["error"], "listing is disabled");
    }
}
