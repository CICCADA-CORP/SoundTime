//! Setup/onboarding API — first-time instance configuration

use axum::{
    extract::State,
    http::StatusCode,
    Extension, Json,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::jwt::generate_token_pair;
use crate::auth::middleware::AuthUser;
use crate::auth::password::hash_password;
use crate::auth::routes::{AuthResponse, ErrorResponse, UserResponse};
use soundtime_db::entities::{instance_setting, user};
use soundtime_db::AppState;

// ─── DTOs ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SetupStatusResponse {
    pub setup_complete: bool,
    pub has_admin: bool,
    pub instance_private: bool,
}

#[derive(Deserialize)]
pub struct SetupAdminRequest {
    pub username: String,
    pub email: String,
    pub password: String,
}

#[derive(Deserialize)]
pub struct SetupInstanceRequest {
    pub instance_name: String,
    pub instance_description: String,
}

#[derive(Deserialize)]
pub struct SetupCompleteRequest {
    pub p2p_enabled: bool,
    pub open_registrations: bool,
    pub max_upload_size_mb: u32,
}

// ─── Helpers ────────────────────────────────────────────────────────

async fn is_setup_complete(
    db: &sea_orm::DatabaseConnection,
) -> Result<bool, StatusCode> {
    let setting = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("setup_complete"))
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(setting.map(|s| s.value == "true").unwrap_or(false))
}

async fn upsert_setting(
    db: &sea_orm::DatabaseConnection,
    key: &str,
    value: &str,
) -> Result<(), StatusCode> {
    let now = chrono::Utc::now().fixed_offset();
    let existing = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq(key))
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(existing) = existing {
        let mut am: instance_setting::ActiveModel = existing.into();
        am.value = Set(value.to_string());
        am.updated_at = Set(now);
        am.update(db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    } else {
        let am = instance_setting::ActiveModel {
            id: Set(Uuid::new_v4()),
            key: Set(key.to_string()),
            value: Set(value.to_string()),
            updated_at: Set(now),
        };
        am.insert(db).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(())
}

// ─── Handlers ───────────────────────────────────────────────────────

/// GET /api/setup/status — public, no auth
pub async fn setup_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SetupStatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    let setup_complete = is_setup_complete(&state.db).await.map_err(|s| {
        (s, Json(ErrorResponse { error: "Internal server error".to_string() }))
    })?;

    let user_count = user::Entity::find()
        .count(&state.db)
        .await
        .map_err(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Internal server error".to_string() }))
        })?;

    let instance_private = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("instance_private"))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value == "true")
        .unwrap_or(false);

    Ok(Json(SetupStatusResponse {
        setup_complete,
        has_admin: user_count > 0,
        instance_private,
    }))
}

/// POST /api/setup/admin — public, but only works if 0 users exist
pub async fn setup_admin(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SetupAdminRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Guard: only if no users exist
    let user_count = user::Entity::find()
        .count(&state.db)
        .await
        .unwrap_or(1);

    if user_count > 0 {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Admin account already exists".to_string(),
            }),
        ));
    }

    // Validate
    if body.username.len() < 3 || body.username.len() > 64 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Username must be between 3 and 64 characters".to_string(),
            }),
        ));
    }
    if body.password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Password must be at least 8 characters".to_string(),
            }),
        ));
    }

    let password_hash = hash_password(&body.password).map_err(|e| {
        tracing::error!("hash error: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Internal server error".to_string() }))
    })?;

    let now = chrono::Utc::now().fixed_offset();
    let user_id = Uuid::new_v4();

    let new_user = user::ActiveModel {
        id: Set(user_id),
        username: Set(body.username.clone()),
        email: Set(body.email.clone()),
        password_hash: Set(password_hash),
        display_name: Set(None),
        avatar_url: Set(None),
        role: Set(user::UserRole::Admin),
        is_banned: Set(false),
        ban_reason: Set(None),
        banned_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    };

    let created = new_user.insert(&state.db).await.map_err(|e| {
        tracing::error!("insert error: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Failed to create admin user".to_string() }))
    })?;

    let tokens = generate_token_pair(
        created.id,
        &created.username,
        created.role.as_str(),
        &state.jwt_secret,
    )
    .map_err(|e| {
        tracing::error!("token error: {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Failed to generate tokens".to_string() }))
    })?;

    tracing::info!("Setup: admin user '{}' created", created.username);

    Ok((
        StatusCode::CREATED,
        Json(AuthResponse {
            user: UserResponse {
                id: created.id,
                instance_id: format!("{}@{}", &created.username, &state.domain),
                username: created.username,
                email: created.email,
                display_name: created.display_name,
                avatar_url: created.avatar_url,
                role: created.role.to_string(),
            },
            tokens,
        }),
    ))
}

/// POST /api/setup/instance — requires admin auth
pub async fn setup_instance(
    State(state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    Json(body): Json<SetupInstanceRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    // SECURITY: only admin users can configure the instance
    if auth_user.0.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse { error: "Admin access required".to_string() }),
        ));
    }
    let setup_complete = is_setup_complete(&state.db).await.map_err(|s| {
        (s, Json(ErrorResponse { error: "Internal server error".to_string() }))
    })?;
    if setup_complete {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse { error: "Setup already complete".to_string() }),
        ));
    }

    upsert_setting(&state.db, "instance_name", &body.instance_name).await.map_err(|s| {
        (s, Json(ErrorResponse { error: "Failed to save settings".to_string() }))
    })?;
    upsert_setting(&state.db, "instance_description", &body.instance_description).await.map_err(|s| {
        (s, Json(ErrorResponse { error: "Failed to save settings".to_string() }))
    })?;

    tracing::info!("Setup: instance configured as '{}'", body.instance_name);

    Ok(Json(serde_json::json!({ "ok": true })))
}

/// POST /api/setup/complete — requires admin auth, finalizes setup
pub async fn setup_complete(
    State(state): State<Arc<AppState>>,
    Extension(auth_user): Extension<AuthUser>,
    Json(body): Json<SetupCompleteRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    // SECURITY: only admin users can finalize setup
    if auth_user.0.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse { error: "Admin access required".to_string() }),
        ));
    }
    let already_complete = is_setup_complete(&state.db).await.map_err(|s| {
        (s, Json(ErrorResponse { error: "Internal server error".to_string() }))
    })?;
    if already_complete {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse { error: "Setup already complete".to_string() }),
        ));
    }

    let err = |s| (s, Json(ErrorResponse { error: "Failed to save settings".to_string() }));
    upsert_setting(&state.db, "p2p_enabled", if body.p2p_enabled { "true" } else { "false" }).await.map_err(err)?;
    upsert_setting(&state.db, "open_registrations", if body.open_registrations { "true" } else { "false" }).await.map_err(err)?;
    upsert_setting(&state.db, "max_upload_size_mb", &body.max_upload_size_mb.to_string()).await.map_err(err)?;
    upsert_setting(&state.db, "setup_complete", "true").await.map_err(err)?;

    tracing::info!("Setup complete! Instance is ready.");

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setup_status_response_serialization() {
        let resp = SetupStatusResponse {
            setup_complete: false,
            has_admin: false,
            instance_private: false,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["setup_complete"], false);
        assert_eq!(json["has_admin"], false);
        assert_eq!(json["instance_private"], false);
    }

    #[test]
    fn test_setup_admin_request_deserialization() {
        let json = r#"{"username": "admin", "email": "admin@example.com", "password": "secret123"}"#;
        let req: SetupAdminRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.username, "admin");
        assert_eq!(req.email, "admin@example.com");
        assert_eq!(req.password, "secret123");
    }

    #[test]
    fn test_setup_instance_request_deserialization() {
        let json = r#"{"instance_name": "My Instance", "instance_description": "A music server"}"#;
        let req: SetupInstanceRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.instance_name, "My Instance");
    }

    #[test]
    fn test_setup_complete_request_deserialization() {
        let json = r#"{"p2p_enabled": true, "open_registrations": false, "max_upload_size_mb": 50}"#;
        let req: SetupCompleteRequest = serde_json::from_str(json).unwrap();
        assert!(req.p2p_enabled);
        assert!(!req.open_registrations);
        assert_eq!(req.max_upload_size_mb, 50);
    }

    #[test]
    fn test_error_response_serialization() {
        let resp = ErrorResponse {
            error: "Something went wrong".into(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["error"], "Something went wrong");
    }
}
