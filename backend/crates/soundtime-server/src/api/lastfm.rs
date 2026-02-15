//! Last.fm scrobbling integration.
//!
//! Provides handlers for connecting a Last.fm account, toggling scrobbling,
//! sending "Now Playing" updates, and scrobbling tracks (fire-and-forget from
//! `log_listen`).

use axum::{extract::State, http::StatusCode, Json};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use soundtime_db::entities::{album, artist, track, user_setting};
use soundtime_db::AppState;

// SECURITY: These are HKDF domain-separation parameters (salt and info), NOT secret keys.
// They ensure the derived encryption key is unique to the "Last.fm session key" use-case.
// The actual secret input to HKDF is `jwt_secret`, sourced from the JWT_SECRET environment variable.
const HKDF_SALT: &[u8] = b"soundtime-lastfm";
const HKDF_INFO: &[u8] = b"lastfm-session-key";

// ─── Structs ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct LastfmConnectResponse {
    pub auth_url: String,
}

#[derive(Debug, Deserialize)]
pub struct LastfmCallbackRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct LastfmStatusResponse {
    pub connected: bool,
    pub username: Option<String>,
    pub scrobble_enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct LastfmToggleRequest {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct LastfmNowPlayingRequest {
    pub track_id: Uuid,
}

// ─── Helpers ────────────────────────────────────────────────────────────

/// Build the `api_sig` required by the Last.fm API protocol.
///
/// # Security
/// MD5 is used here because the Last.fm API *requires* it for request signing.
/// This is NOT a security choice — it is a protocol requirement.
// SECURITY: MD5 used only for Last.fm api_sig (protocol requirement, not a security choice)
fn build_api_sig(params: &BTreeMap<&str, &str>, secret: &str) -> String {
    let mut sig_input = String::new();
    for (k, v) in params {
        sig_input.push_str(k);
        sig_input.push_str(v);
    }
    sig_input.push_str(secret);
    format!("{:x}", md5::compute(sig_input.as_bytes()))
}

/// Encrypt a Last.fm session key for storage using AES-256-GCM.
///
/// The encryption key is derived from `jwt_secret` via HKDF-SHA256.
/// Returns base64-encoded `nonce || ciphertext`.
fn encrypt_session_key(key: &str, jwt_secret: &str) -> Result<String, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
    use hkdf::Hkdf;
    use sha2::Sha256;

    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), jwt_secret.as_bytes());
    let mut derived = [0u8; 32];
    hk.expand(HKDF_INFO, &mut derived)
        .map_err(|e| format!("HKDF expand failed: {e}"))?;

    let cipher =
        Aes256Gcm::new_from_slice(&derived).map_err(|e| format!("AES-GCM key init failed: {e}"))?;

    let nonce_bytes: [u8; 12] = rand::random();
    #[allow(deprecated)]
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, key.as_bytes())
        .map_err(|e| format!("Encryption failed: {e}"))?;

    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(&combined))
}

/// Decrypt a stored Last.fm session key.
fn decrypt_session_key(encrypted: &str, jwt_secret: &str) -> Result<String, String> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
    use hkdf::Hkdf;
    use sha2::Sha256;

    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), jwt_secret.as_bytes());
    let mut derived = [0u8; 32];
    hk.expand(HKDF_INFO, &mut derived)
        .map_err(|e| format!("HKDF expand failed: {e}"))?;

    let cipher =
        Aes256Gcm::new_from_slice(&derived).map_err(|e| format!("AES-GCM key init failed: {e}"))?;

    use base64::Engine;
    let combined = base64::engine::general_purpose::STANDARD
        .decode(encrypted)
        .map_err(|e| format!("Base64 decode failed: {e}"))?;

    if combined.len() < 12 {
        return Err("Ciphertext too short".to_string());
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    #[allow(deprecated)]
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decryption failed: {e}"))?;

    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 decode failed: {e}"))
}

/// Helper: get a user setting by key.
async fn get_user_setting(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
    key: &str,
) -> Option<String> {
    user_setting::Entity::find()
        .filter(user_setting::Column::UserId.eq(user_id))
        .filter(user_setting::Column::Key.eq(key))
        .one(db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value)
}

/// Helper: upsert a user setting.
async fn set_user_setting(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
    key: &str,
    value: &str,
) -> Result<(), sea_orm::DbErr> {
    let existing = user_setting::Entity::find()
        .filter(user_setting::Column::UserId.eq(user_id))
        .filter(user_setting::Column::Key.eq(key))
        .one(db)
        .await?;

    if let Some(existing) = existing {
        let mut model: user_setting::ActiveModel = existing.into();
        model.value = Set(value.to_string());
        model.updated_at = Set(chrono::Utc::now().fixed_offset());
        model.update(db).await?;
    } else {
        let model = user_setting::ActiveModel {
            id: Set(Uuid::new_v4()),
            user_id: Set(user_id),
            key: Set(key.to_string()),
            value: Set(value.to_string()),
            updated_at: Set(chrono::Utc::now().fixed_offset()),
        };
        model.insert(db).await?;
    }

    Ok(())
}

/// Helper: delete all Last.fm settings for a user.
async fn delete_lastfm_settings(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
) -> Result<(), sea_orm::DbErr> {
    user_setting::Entity::delete_many()
        .filter(user_setting::Column::UserId.eq(user_id))
        .filter(user_setting::Column::Key.like("lastfm_%"))
        .exec(db)
        .await?;
    Ok(())
}

// ─── Handlers ───────────────────────────────────────────────────────────

/// GET /api/lastfm/status
pub async fn lastfm_status(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
) -> Result<Json<LastfmStatusResponse>, (StatusCode, String)> {
    let user_id = auth_user.0.sub;

    let username = get_user_setting(&state.db, user_id, "lastfm_username").await;
    let scrobble_enabled = get_user_setting(&state.db, user_id, "lastfm_scrobble_enabled")
        .await
        .map(|v| v == "true")
        .unwrap_or(false);

    Ok(Json(LastfmStatusResponse {
        connected: username.is_some(),
        username,
        scrobble_enabled,
    }))
}

/// GET /api/lastfm/connect
pub async fn lastfm_connect(
    State(state): State<Arc<AppState>>,
    axum::Extension(_auth_user): axum::Extension<AuthUser>,
) -> Result<Json<LastfmConnectResponse>, (StatusCode, String)> {
    let api_key = std::env::var("LASTFM_API_KEY").map_err(|_| {
        (
            StatusCode::NOT_IMPLEMENTED,
            "Last.fm integration is not configured on this instance".to_string(),
        )
    })?;

    let callback_url = {
        let scheme = std::env::var("SOUNDTIME_SCHEME").unwrap_or_else(|_| "https".to_string());
        format!("{scheme}://{}/settings?lastfm_callback=1", state.domain)
    };

    let auth_url = format!(
        "https://www.last.fm/api/auth/?api_key={}&cb={}",
        api_key,
        urlencoding::encode(&callback_url)
    );

    Ok(Json(LastfmConnectResponse { auth_url }))
}

/// POST /api/lastfm/callback
pub async fn lastfm_callback(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Json(body): Json<LastfmCallbackRequest>,
) -> Result<Json<LastfmStatusResponse>, (StatusCode, String)> {
    let api_key = std::env::var("LASTFM_API_KEY").map_err(|_| {
        (
            StatusCode::NOT_IMPLEMENTED,
            "Last.fm integration is not configured".to_string(),
        )
    })?;
    let api_secret = std::env::var("LASTFM_API_SECRET").map_err(|_| {
        (
            StatusCode::NOT_IMPLEMENTED,
            "Last.fm API secret is not configured".to_string(),
        )
    })?;

    // Build auth.getSession request
    let mut params = BTreeMap::new();
    params.insert("api_key", api_key.as_str());
    params.insert("method", "auth.getSession");
    params.insert("token", body.token.as_str());
    let api_sig = build_api_sig(&params, &api_secret);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("HTTP client error: {e}"),
            )
        })?;

    let resp = client
        .get("https://ws.audioscrobbler.com/2.0/")
        .query(&[
            ("method", "auth.getSession"),
            ("api_key", &api_key),
            ("token", &body.token),
            ("api_sig", &api_sig),
            ("format", "json"),
        ])
        .send()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Last.fm auth.getSession request failed");
            (
                StatusCode::BAD_GATEWAY,
                format!("Failed to contact Last.fm: {e}"),
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        tracing::warn!(status = %status, body = %body_text, "Last.fm auth.getSession failed");
        return Err((
            StatusCode::BAD_GATEWAY,
            "Failed to authenticate with Last.fm".to_string(),
        ));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("Failed to parse Last.fm response: {e}"),
        )
    })?;

    let session = json.get("session").ok_or((
        StatusCode::BAD_GATEWAY,
        "No session in Last.fm response".to_string(),
    ))?;

    let session_key = session.get("key").and_then(|v| v.as_str()).ok_or((
        StatusCode::BAD_GATEWAY,
        "No session key in Last.fm response".to_string(),
    ))?;

    let username = session.get("name").and_then(|v| v.as_str()).ok_or((
        StatusCode::BAD_GATEWAY,
        "No username in Last.fm response".to_string(),
    ))?;

    // Encrypt and store
    let encrypted = encrypt_session_key(session_key, &state.jwt_secret).map_err(|e| {
        tracing::error!(error = %e, "Failed to encrypt Last.fm session key");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Encryption error".to_string(),
        )
    })?;

    let user_id = auth_user.0.sub;
    set_user_setting(&state.db, user_id, "lastfm_session_key", &encrypted)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
    set_user_setting(&state.db, user_id, "lastfm_username", username)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;
    set_user_setting(&state.db, user_id, "lastfm_scrobble_enabled", "true")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    tracing::info!(user_id = %user_id, lastfm_user = %username, "Last.fm account connected");

    Ok(Json(LastfmStatusResponse {
        connected: true,
        username: Some(username.to_string()),
        scrobble_enabled: true,
    }))
}

/// POST /api/lastfm/toggle
pub async fn lastfm_toggle(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Json(body): Json<LastfmToggleRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let user_id = auth_user.0.sub;

    set_user_setting(
        &state.db,
        user_id,
        "lastfm_scrobble_enabled",
        if body.enabled { "true" } else { "false" },
    )
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(StatusCode::OK)
}

/// DELETE /api/lastfm/disconnect
pub async fn lastfm_disconnect(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
) -> Result<StatusCode, (StatusCode, String)> {
    let user_id = auth_user.0.sub;

    delete_lastfm_settings(&state.db, user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    tracing::info!(user_id = %user_id, "Last.fm account disconnected");

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/lastfm/now-playing
pub async fn lastfm_now_playing(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Json(body): Json<LastfmNowPlayingRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let api_key = match std::env::var("LASTFM_API_KEY") {
        Ok(k) => k,
        Err(_) => return Ok(StatusCode::OK), // silently ignore if not configured
    };
    let api_secret = match std::env::var("LASTFM_API_SECRET") {
        Ok(s) => s,
        Err(_) => return Ok(StatusCode::OK),
    };

    let user_id = auth_user.0.sub;

    // Check if scrobbling is enabled
    let enabled = get_user_setting(&state.db, user_id, "lastfm_scrobble_enabled")
        .await
        .map(|v| v == "true")
        .unwrap_or(false);
    if !enabled {
        return Ok(StatusCode::OK);
    }

    // Get encrypted session key
    let encrypted_sk = match get_user_setting(&state.db, user_id, "lastfm_session_key").await {
        Some(sk) => sk,
        None => return Ok(StatusCode::OK),
    };

    let session_key = decrypt_session_key(&encrypted_sk, &state.jwt_secret).map_err(|e| {
        tracing::warn!(error = %e, "Failed to decrypt Last.fm session key");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Decryption error".to_string(),
        )
    })?;

    // Get track info
    let track_model = track::Entity::find_by_id(body.track_id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "Track not found".to_string()))?;

    let artist_model = artist::Entity::find_by_id(track_model.artist_id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    let artist_name = artist_model.map(|a| a.name).unwrap_or_default();
    if artist_name.is_empty() || track_model.title.is_empty() {
        return Ok(StatusCode::OK);
    }

    let album_title = if let Some(album_id) = track_model.album_id {
        album::Entity::find_by_id(album_id)
            .one(&state.db)
            .await
            .ok()
            .flatten()
            .map(|a| a.title)
    } else {
        None
    };

    let duration_str = (track_model.duration_secs as u64).to_string();

    // Build Now Playing request
    let mut params = BTreeMap::new();
    params.insert("method", "track.updateNowPlaying");
    params.insert("api_key", api_key.as_str());
    params.insert("sk", session_key.as_str());
    params.insert("artist", artist_name.as_str());
    params.insert("track", track_model.title.as_str());
    params.insert("duration", duration_str.as_str());
    if let Some(ref album) = album_title {
        params.insert("album", album.as_str());
    }
    let api_sig = build_api_sig(&params, &api_secret);

    // Fire and forget (best-effort)
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("HTTP client error: {e}"),
            )
        })?;

    let mut form_params: Vec<(&str, &str)> = params.into_iter().collect();
    form_params.push(("api_sig", api_sig.as_str()));
    form_params.push(("format", "json"));

    let resp = client
        .post("https://ws.audioscrobbler.com/2.0/")
        .form(&form_params)
        .send()
        .await;

    match resp {
        Ok(r) if !r.status().is_success() => {
            tracing::warn!(status = %r.status(), "Last.fm now playing update failed");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Last.fm now playing request failed");
        }
        _ => {}
    }

    Ok(StatusCode::OK)
}

// ─── Scrobble (called from log_listen) ──────────────────────────────────

/// Scrobble a track to Last.fm for a given user (fire-and-forget).
///
/// Called from `log_listen` via `tokio::spawn`. Errors are logged, never returned
/// to the HTTP response.
pub async fn scrobble_for_user(
    db: &sea_orm::DatabaseConnection,
    user_id: Uuid,
    track_id: Uuid,
    duration_listened: f32,
    timestamp: chrono::DateTime<chrono::FixedOffset>,
    jwt_secret: &str,
) -> Result<(), String> {
    // Check env vars
    let api_key = match std::env::var("LASTFM_API_KEY") {
        Ok(k) => k,
        Err(_) => return Ok(()), // Not configured
    };
    let api_secret = match std::env::var("LASTFM_API_SECRET") {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    // Check if scrobbling is enabled for this user
    let enabled = get_user_setting(db, user_id, "lastfm_scrobble_enabled")
        .await
        .map(|v| v == "true")
        .unwrap_or(false);
    if !enabled {
        return Ok(());
    }

    // Get encrypted session key
    let encrypted_sk = match get_user_setting(db, user_id, "lastfm_session_key").await {
        Some(sk) => sk,
        None => return Ok(()),
    };

    let session_key = decrypt_session_key(&encrypted_sk, jwt_secret)?;

    // Get track info
    let track_model = track::Entity::find_by_id(track_id)
        .one(db)
        .await
        .map_err(|e| format!("DB error: {e}"))?
        .ok_or_else(|| "Track not found".to_string())?;

    // Check scrobble eligibility: >= 30s OR >= half of track duration
    let half_duration = track_model.duration_secs / 2.0;
    if duration_listened < 30.0 && duration_listened < half_duration {
        return Ok(());
    }

    let artist_model = artist::Entity::find_by_id(track_model.artist_id)
        .one(db)
        .await
        .map_err(|e| format!("DB error: {e}"))?;

    let artist_name = artist_model.map(|a| a.name).unwrap_or_default();

    if artist_name.is_empty() {
        tracing::warn!(track_id = %track_id, "Skipping scrobble: no artist name");
        return Ok(());
    }
    if track_model.title.is_empty() {
        tracing::warn!(track_id = %track_id, "Skipping scrobble: empty track title");
        return Ok(());
    }

    let album_title = if let Some(album_id) = track_model.album_id {
        album::Entity::find_by_id(album_id)
            .one(db)
            .await
            .ok()
            .flatten()
            .map(|a| a.title)
    } else {
        None
    };

    let timestamp_str = timestamp.timestamp().to_string();
    let duration_str = (track_model.duration_secs as u64).to_string();

    // Build scrobble request
    let mut params = BTreeMap::new();
    params.insert("method", "track.scrobble");
    params.insert("api_key", api_key.as_str());
    params.insert("sk", session_key.as_str());
    params.insert("artist", artist_name.as_str());
    params.insert("track", track_model.title.as_str());
    params.insert("timestamp", timestamp_str.as_str());
    params.insert("duration", duration_str.as_str());
    if let Some(ref album) = album_title {
        params.insert("album", album.as_str());
    }
    let api_sig = build_api_sig(&params, &api_secret);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let mut form_params: Vec<(&str, &str)> = params.into_iter().collect();
    form_params.push(("api_sig", api_sig.as_str()));
    form_params.push(("format", "json"));

    let resp = client
        .post("https://ws.audioscrobbler.com/2.0/")
        .form(&form_params)
        .send()
        .await
        .map_err(|e| format!("Last.fm request failed: {e}"))?;

    if resp.status() == reqwest::StatusCode::FORBIDDEN {
        // Session key revoked — clean up user settings
        tracing::warn!(user_id = %user_id, "Last.fm session revoked (403), cleaning up settings");
        let _ = delete_lastfm_settings(db, user_id).await;
        return Err("Last.fm session revoked".to_string());
    }

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        tracing::warn!(user_id = %user_id, status = %status, body = %body_text, "Last.fm scrobble failed");
        return Err(format!("Last.fm scrobble failed: {status}"));
    }

    tracing::info!(
        user_id = %user_id,
        track = %track_model.title,
        artist = %artist_name,
        "Scrobbled to Last.fm"
    );

    Ok(())
}

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_api_sig() {
        let mut params = BTreeMap::new();
        params.insert("api_key", "test_key");
        params.insert("method", "auth.getSession");
        params.insert("token", "test_token");
        let sig = build_api_sig(&params, "test_secret");
        // MD5 of "api_keytest_keymethodauth.getSessiontokentest_tokentest_secret"
        assert_eq!(sig.len(), 32); // MD5 hex length
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_encrypt_decrypt_session_key() {
        let key = "session-key-12345";
        let secret = "my-jwt-secret-for-testing";

        let encrypted = encrypt_session_key(key, secret).unwrap();
        assert_ne!(encrypted, key); // Must be different

        let decrypted = decrypt_session_key(&encrypted, secret).unwrap();
        assert_eq!(decrypted, key);
    }

    #[test]
    fn test_decrypt_wrong_secret_fails() {
        let key = "session-key-12345";
        let encrypted = encrypt_session_key(key, "correct-secret").unwrap();
        let result = decrypt_session_key(&encrypted, "wrong-secret");
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_callback_request() {
        let json = r#"{"token":"abc123"}"#;
        let req: LastfmCallbackRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.token, "abc123");
    }

    #[test]
    fn test_deserialize_toggle_request() {
        let json = r#"{"enabled":true}"#;
        let req: LastfmToggleRequest = serde_json::from_str(json).unwrap();
        assert!(req.enabled);
    }

    #[test]
    fn test_deserialize_now_playing_request() {
        let json = r#"{"track_id":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let req: LastfmNowPlayingRequest = serde_json::from_str(json).unwrap();
        assert_eq!(
            req.track_id.to_string(),
            "550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_serialize_status_response() {
        let resp = LastfmStatusResponse {
            connected: true,
            username: Some("testuser".to_string()),
            scrobble_enabled: true,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["connected"], true);
        assert_eq!(json["username"], "testuser");
        assert_eq!(json["scrobble_enabled"], true);
    }

    #[test]
    fn test_serialize_connect_response() {
        let resp = LastfmConnectResponse {
            auth_url: "https://www.last.fm/api/auth/?api_key=test".to_string(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert!(json["auth_url"].as_str().unwrap().starts_with("https://"));
    }
}
