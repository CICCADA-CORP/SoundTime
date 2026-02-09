//! Listing worker — periodically announces this instance to the public SoundTime node directory.
//!
//! When the admin enables `listing_public` in instance settings,
//! a heartbeat is sent every 5 minutes to the listing server.
//! The listing server checks node health and removes offline nodes after 48h.

use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use soundtime_db::entities::instance_setting;
use soundtime_db::AppState;
use std::sync::Arc;
use uuid::Uuid;

/// How often to send a heartbeat (5 minutes).
const HEARTBEAT_INTERVAL_SECS: u64 = 300;

/// Default listing server URL.
const DEFAULT_LISTING_URL: &str = "https://listing.soundtime.app";

/// Spawn the listing heartbeat worker.
pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        tracing::info!("listing worker started (heartbeat every 5m)");

        // Wait 30s before first attempt to let the server fully start
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;

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
                if let Err(e) = send_heartbeat(&state, &client, &listing_url).await {
                    tracing::warn!("listing heartbeat failed: {e}");
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(HEARTBEAT_INTERVAL_SECS)).await;
        }
    });
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
    let domain = &state.domain;
    let name = get_instance_name(state).await;
    let description = get_instance_description(state).await;
    let token = get_listing_token(state).await;

    let mut payload = serde_json::json!({
        "domain": domain,
        "name": name,
        "description": description,
        "version": "0.1.0",
    });

    // Include token for heartbeat (subsequent calls)
    if let Some(ref t) = token {
        payload["token"] = serde_json::Value::String(t.clone());
    }

    let url = format!("{listing_url}/api/announce");
    tracing::debug!("sending listing heartbeat to {url}");

    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    if status.is_success() {
        // If this was a new registration, save the token
        if token.is_none() {
            if let Some(new_token) = body.get("token").and_then(|t| t.as_str()) {
                tracing::info!("registered on listing server — saving token");
                save_listing_token(state, new_token).await;
            }
        }
        tracing::debug!("listing heartbeat successful: {}", body);
        Ok(())
    } else {
        let error = body
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("unknown error");
        Err(format!("Listing server returned {status}: {error}"))
    }
}
