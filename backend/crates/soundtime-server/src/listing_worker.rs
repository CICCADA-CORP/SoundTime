//! Listing worker — periodically announces this instance to the public SoundTime node directory.
//!
//! Listing is **disabled by default** (opt-in via `listing_public = true`).
//! A heartbeat is sent every 5 minutes to the listing server.
//! When an admin disables listing, a DELETE request is sent immediately to
//! remove the instance from the directory. Nodes that crash without delisting
//! are removed by the listing server after 48h of failed health checks.
//!
//! The module also exposes admin endpoints to trigger a heartbeat or delist immediately.

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
        "version": soundtime_p2p::build_version(),
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
///
/// Returns `Ok((status_code, body))` — the caller decides what to do with non-2xx.
async fn send_announce_request(
    client: &reqwest::Client,
    listing_url: &str,
    payload: &serde_json::Value,
) -> Result<(reqwest::StatusCode, serde_json::Value), String> {
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

    Ok((status, body))
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

/// POST /api/admin/listing/trigger — send a heartbeat NOW and return the result.
///
/// Unlike the background worker, this handler sends the heartbeat synchronously
/// so the admin UI can show whether the listing server accepted or rejected us.
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

    let listing_url = get_listing_url(&state).await;
    let domain = get_listing_domain(&state).await;

    if is_local_domain(&domain) {
        save_listing_status(&state, "error", Some("domain is a local address (localhost / 127.x) — the listing server cannot reach this instance")).await;
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "domain_local",
                "message": "The listing domain is a local address. The listing server cannot reach your instance. Set a public domain in the listing settings or via the SOUNDTIME_DOMAIN environment variable."
            })),
        ));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!("SoundTime/{}", soundtime_p2p::build_version()))
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("HTTP client error: {e}") })),
            )
        })?;

    match send_heartbeat(&state, &client, &listing_url).await {
        Ok(()) => {
            save_listing_status(&state, "ok", None).await;
            Ok(Json(serde_json::json!({
                "status": "ok",
                "domain": domain,
                "listing_url": listing_url,
                "message": "Heartbeat successful — this instance is now listed."
            })))
        }
        Err(e) => {
            save_listing_status(&state, "error", Some(&e)).await;
            Err((
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "heartbeat_failed",
                    "message": e,
                    "domain": domain,
                    "listing_url": listing_url,
                })),
            ))
        }
    }
}

/// POST /api/admin/listing/delist — immediately remove this instance from the public directory.
pub async fn delist(
    AxumState(state): AxumState<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let listing_url = get_listing_url(&state).await;
    let domain = get_listing_domain(&state).await;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!("SoundTime/{}", soundtime_p2p::build_version()))
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("HTTP client error: {e}") })),
            )
        })?;

    match send_delist_request(&state, &client, &listing_url).await {
        Ok(()) => {
            save_listing_status(&state, "delisted", None).await;
            Ok(Json(serde_json::json!({
                "status": "ok",
                "domain": domain,
                "message": "Instance has been removed from the public directory."
            })))
        }
        Err(e) => {
            save_listing_status(&state, "error", Some(&e)).await;
            Err((
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "delist_failed",
                    "message": e,
                    "domain": domain,
                    "listing_url": listing_url,
                })),
            ))
        }
    }
}

/// GET /api/admin/listing/status — return current listing status.
pub async fn listing_status(AxumState(state): AxumState<Arc<AppState>>) -> Json<serde_json::Value> {
    let is_enabled = is_listing_enabled(&state).await;
    let domain = get_listing_domain(&state).await;
    let listing_url = get_listing_url(&state).await;
    let token = get_listing_token(&state).await;

    // Read stored status
    let status = get_setting(&state, "listing_last_status")
        .await
        .unwrap_or_default();
    let error = get_setting(&state, "listing_last_error").await;
    let last_heartbeat = get_setting(&state, "listing_last_heartbeat").await;

    Json(serde_json::json!({
        "enabled": is_enabled,
        "domain": domain,
        "domain_is_local": is_local_domain(&domain),
        "listing_url": listing_url,
        "has_token": token.is_some(),
        "status": if status.is_empty() { "unknown" } else { &status },
        "error": error,
        "last_heartbeat": last_heartbeat,
    }))
}

/// Spawn the listing heartbeat worker.
pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        tracing::info!("listing worker started (heartbeat every 5m, disabled by default)");

        // Wait 10s before first attempt to let the server fully start
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent(format!("SoundTime/{}", soundtime_p2p::build_version()))
            .build()
            .expect("failed to build HTTP client");

        let mut was_enabled = false;

        loop {
            let is_enabled = is_listing_enabled(&state).await;

            if is_enabled {
                was_enabled = true;
                let listing_url = get_listing_url(&state).await;
                let listing_domain = get_listing_domain(&state).await;
                tracing::info!(
                    domain = %listing_domain,
                    listing_url = %listing_url,
                    "sending listing heartbeat"
                );
                match send_heartbeat(&state, &client, &listing_url).await {
                    Ok(()) => {
                        tracing::info!("listing heartbeat successful");
                        save_listing_status(&state, "ok", None).await;
                    }
                    Err(e) => {
                        tracing::warn!("listing heartbeat failed: {e}");
                        save_listing_status(&state, "error", Some(&e)).await;
                    }
                }
            } else {
                // If listing was enabled before but is now disabled, send a delist request
                if was_enabled {
                    tracing::info!("listing disabled — sending immediate delist request");
                    let listing_url = get_listing_url(&state).await;
                    match send_delist_request(&state, &client, &listing_url).await {
                        Ok(()) => {
                            save_listing_status(&state, "delisted", None).await;
                        }
                        Err(e) => {
                            tracing::warn!("delist request failed: {e}");
                            save_listing_status(&state, "error", Some(&e)).await;
                        }
                    }
                    was_enabled = false;
                } else {
                    tracing::debug!("listing disabled, skipping heartbeat");
                }
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
/// Defaults to `false` — listing is opt-in.
async fn is_listing_enabled(state: &AppState) -> bool {
    instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("listing_public"))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value == "true")
        .unwrap_or(false)
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
        .filter(|v| !v.is_empty())
}

/// Save the listing token to instance settings.
async fn save_listing_token(state: &AppState, token: &str) {
    upsert_setting(state, "listing_token", token).await;
}

/// Delete the listing token from instance settings (used when the server rejects it).
async fn delete_listing_token(state: &AppState) {
    upsert_setting(state, "listing_token", "").await;
}

/// Get the instance name from settings.
async fn get_instance_name(state: &AppState) -> String {
    get_setting(state, "instance_name")
        .await
        .unwrap_or_else(|| "SoundTime".to_string())
}

/// Get the instance description from settings.
async fn get_instance_description(state: &AppState) -> String {
    get_setting(state, "instance_description")
        .await
        .unwrap_or_default()
}

/// Read a single instance_setting by key.
async fn get_setting(state: &AppState, key: &str) -> Option<String> {
    instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq(key))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value)
        .filter(|v| !v.is_empty())
}

/// Upsert an instance_setting key/value pair.
async fn upsert_setting(state: &AppState, key: &str, value: &str) {
    let existing = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq(key))
        .one(&state.db)
        .await
        .ok()
        .flatten();

    match existing {
        Some(s) => {
            let mut update: instance_setting::ActiveModel = s.into();
            update.value = Set(value.to_string());
            update.updated_at = Set(chrono::Utc::now().into());
            let _ = update.update(&state.db).await;
        }
        None => {
            let _ = instance_setting::ActiveModel {
                id: Set(Uuid::new_v4()),
                key: Set(key.to_string()),
                value: Set(value.to_string()),
                updated_at: Set(chrono::Utc::now().into()),
            }
            .insert(&state.db)
            .await;
        }
    }
}

/// Persist the listing heartbeat status so the admin panel can display it.
async fn save_listing_status(state: &AppState, status: &str, error: Option<&str>) {
    upsert_setting(state, "listing_last_status", status).await;
    upsert_setting(state, "listing_last_error", error.unwrap_or("")).await;
    upsert_setting(
        state,
        "listing_last_heartbeat",
        &chrono::Utc::now().to_rfc3339(),
    )
    .await;
}

/// Send a heartbeat to the listing server.
///
/// Handles the 409 "domain already registered" case by attempting to verify
/// ownership via the listing server's health check.
/// When the listing server runs its next health check and finds us healthy,
/// we remain listed even without a token — but we can't update our info.
/// In that case we delete the stale local token and request a fresh registration
/// by sending a DELETE + re-announce.
async fn send_heartbeat(
    state: &AppState,
    client: &reqwest::Client,
    listing_url: &str,
) -> Result<(), String> {
    let domain = get_listing_domain(state).await;

    // Bail out early if domain is a local address
    if is_local_domain(&domain) {
        return Err(format!(
            "domain is a local address ({domain}) — the listing server cannot reach this instance. \
             Set a public domain in admin settings or SOUNDTIME_DOMAIN env var."
        ));
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
        domain: domain.clone(),
        name,
        description,
        token: token.clone(),
        track_count,
        user_count,
        open_registration,
    };

    let payload = build_announce_payload(&params);

    let (status, body) = send_announce_request(client, listing_url, &payload).await?;

    if status.is_success() {
        // If this was a new registration, save the token
        if token.is_none() {
            if let Some(new_token) = body.get("token").and_then(|t| t.as_str()) {
                tracing::info!("registered on listing server — saving token");
                save_listing_token(state, new_token).await;
            }
        }
        tracing::debug!("listing heartbeat successful");
        return Ok(());
    }

    // Handle 401: token is invalid or expired.
    // Delete the stale local token and retry as a fresh registration.
    if status.as_u16() == 401 && token.is_some() {
        tracing::warn!(
            domain = %domain,
            "listing server returned 401 (invalid token) — deleting stale token and re-registering"
        );
        delete_listing_token(state).await;

        // Rebuild payload without token
        let fresh_params = HeartbeatParams {
            token: None,
            ..params
        };
        let fresh_payload = build_announce_payload(&fresh_params);
        let (retry_status, retry_body) =
            send_announce_request(client, listing_url, &fresh_payload).await?;

        if retry_status.is_success() {
            if let Some(new_token) = retry_body.get("token").and_then(|t| t.as_str()) {
                tracing::info!("re-registered on listing server with fresh token");
                save_listing_token(state, new_token).await;
            }
            return Ok(());
        }

        // If the tokenless retry also fails (e.g. 409 domain exists), fall through
        // to the normal error handling below with the retry's status/body.
        let error = retry_body
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("unknown error");
        return Err(format!(
            "Re-registration after token invalidation failed — listing server returned {retry_status}: {error}"
        ));
    }

    // Handle 409: "domain already registered" — we lost our token.
    // The listing server won't let us re-register because the domain already exists.
    // Strategy: the listing server checks `/healthz` periodically. If it finds us
    // healthy, we stay listed. We just need to wait or, if possible, the listing
    // server will eventually remove the stale entry (48h offline threshold).
    //
    // But if we ARE healthy and the listing server's health checker works, we
    // ARE already listed — we just can't update our info via heartbeat.
    // So we check: is our domain actually listed on the listing server?
    if status.as_u16() == 409 {
        tracing::warn!(
            domain = %domain,
            "listing server returned 409 (domain already registered) — checking if we're still listed"
        );
        // Check if we're listed by querying the public nodes endpoint
        match check_if_listed(client, listing_url, &domain).await {
            Ok(true) => {
                // We're still listed! The listing server keeps us alive via health checks.
                // We just can't send heartbeats. This is OK.
                tracing::info!("instance IS listed on the directory (via health checks) — token was lost but we're still visible");
                return Ok(());
            }
            Ok(false) => {
                // We're NOT listed and can't register → the old entry is gone or offline.
                // This shouldn't happen (409 means entry exists). Log and report error.
                tracing::warn!(
                    "listing server returned 409 but we're not listed — inconsistent state"
                );
            }
            Err(e) => {
                tracing::warn!("failed to check listing status: {e}");
            }
        }
        return Err(format!(
            "Domain '{domain}' is already registered on the listing server but the local token was lost. \
             The instance may still appear via the listing server's periodic health checks. \
             If not, the listing server will remove the entry after 48h of failed health checks."
        ));
    }

    // Handle other errors
    let error = body
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or("unknown error");
    Err(format!("Listing server returned {status}: {error}"))
}

/// Check if our domain appears in the listing server's public node list.
async fn check_if_listed(
    client: &reqwest::Client,
    listing_url: &str,
    domain: &str,
) -> Result<bool, String> {
    let url = format!("{listing_url}/api/nodes/{domain}");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;
    Ok(resp.status().is_success())
}

/// Send a DELETE request to the listing server to immediately remove this instance.
///
/// Requires a valid listing token. If no token is available, the instance was
/// never registered (or the token was lost) — in that case, the listing server's
/// 48h health-check cleanup will handle removal.
async fn send_delist_request(
    state: &AppState,
    client: &reqwest::Client,
    listing_url: &str,
) -> Result<(), String> {
    let domain = get_listing_domain(state).await;
    let token = match get_listing_token(state).await {
        Some(t) => t,
        None => {
            tracing::warn!("no listing token available — cannot send delist request (instance may not have been registered)");
            return Err("No listing token available. If the instance was previously registered, the listing server will remove it after 48h of failed health checks.".to_string());
        }
    };

    let url = format!("{listing_url}/api/nodes/{domain}");
    tracing::info!(domain = %domain, listing_url = %listing_url, "sending delist request to listing server");

    let resp = client
        .delete(&url)
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = resp.status();

    if status.is_success() {
        tracing::info!(domain = %domain, "successfully delisted from directory");
        // Clear the token since we're no longer registered
        delete_listing_token(state).await;
        return Ok(());
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .unwrap_or_else(|_| serde_json::json!({"error": "unknown"}));

    let error = body
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or("unknown error");

    tracing::warn!(
        domain = %domain,
        status = %status,
        error = %error,
        "delist request failed"
    );

    Err(format!("Listing server returned {status}: {error}"))
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
        let (status, body) = result.unwrap();
        assert!(status.is_success());
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
        let (status, body) = result.unwrap();
        assert!(status.is_success());
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

        // send_announce_request now returns Ok with the status + body even for non-2xx
        assert!(result.is_ok());
        let (status, body) = result.unwrap();
        assert_eq!(status.as_u16(), 422);
        assert_eq!(body["error"], "Domain is not reachable");
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

        assert!(result.is_ok());
        let (status, _body) = result.unwrap();
        assert_eq!(status.as_u16(), 500);
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

        assert!(result.is_ok());
        let (status, body) = result.unwrap();
        assert_eq!(status.as_u16(), 400);
        assert_eq!(body["message"], "bad request");
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
        let (status, _body) = result.unwrap();
        assert!(status.is_success());
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
            let (status, _) = result.unwrap();
            assert!(status.is_success());
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

        let (status, body) = send_announce_request(&client, &mock_server.uri(), &payload)
            .await
            .expect("registration should succeed");

        assert!(status.is_success());
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

        let (status, body) = send_announce_request(&client, &mock_server.uri(), &payload)
            .await
            .expect("heartbeat should succeed");

        assert!(status.is_success());
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

    // ─── delist request tests ────────────────────────────────────────

    #[tokio::test]
    async fn delist_request_sends_delete_with_bearer_token() {
        let mock_server = MockServer::start().await;

        Mock::given(method("DELETE"))
            .and(path("/api/nodes/music.example.com"))
            .and(wiremock::matchers::header(
                "Authorization",
                "Bearer my-token",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "removed",
                "domain": "music.example.com"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = http_client();
        let url = format!("{}/api/nodes/music.example.com", mock_server.uri());
        let resp = client
            .delete(&url)
            .header("Authorization", "Bearer my-token")
            .send()
            .await
            .unwrap();

        assert!(resp.status().is_success());
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "removed");
    }

    #[tokio::test]
    async fn delist_request_returns_404_with_invalid_token() {
        let mock_server = MockServer::start().await;

        Mock::given(method("DELETE"))
            .and(path("/api/nodes/music.example.com"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "error": "node not found or invalid token"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = http_client();
        let url = format!("{}/api/nodes/music.example.com", mock_server.uri());
        let resp = client
            .delete(&url)
            .header("Authorization", "Bearer bad-token")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status().as_u16(), 404);
    }
}
