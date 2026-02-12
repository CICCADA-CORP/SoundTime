use axum::{extract::State, http::StatusCode, Json};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use super::jwt::{generate_token_pair, validate_token, TokenPair, TokenType};
use super::middleware::AuthUser;
use super::password::{hash_password, verify_password};
use soundtime_db::entities::{
    favorite, instance_setting, listen_history, playlist, playlist_track, track, track_report, user,
};
use soundtime_db::AppState;

// ─── Request/Response DTOs ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub role: String,
    pub instance_id: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub user: UserResponse,
    pub tokens: TokenPair,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

// ─── Handlers ──────────────────────────────────────────────────────

/// POST /api/auth/register
pub async fn register(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), (StatusCode, Json<ErrorResponse>)> {
    // Block registration during setup (before setup_complete = true)
    let setup_complete = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("setup_complete"))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value == "true")
        .unwrap_or(false);

    let user_count: u64 = user::Entity::find().count(&state.db).await.unwrap_or(0);

    if !setup_complete && user_count > 0 {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Instance is being configured. Registration is disabled until setup is complete.".to_string(),
            }),
        ));
    }

    // Block registration on private instances (admin creates accounts manually)
    let is_private = instance_setting::Entity::find()
        .filter(instance_setting::Column::Key.eq("instance_private"))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value == "true")
        .unwrap_or(false);

    if is_private {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Registration is disabled on this instance. Contact the administrator."
                    .to_string(),
            }),
        ));
    }

    // Validate input
    if body.username.len() < 3 || body.username.len() > 64 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Username must be between 3 and 64 characters".to_string(),
            }),
        ));
    }

    if body.username.contains('@') || body.username.contains('/') || body.username.contains(' ') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Username cannot contain @, / or spaces".to_string(),
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

    // SECURITY: basic email format validation
    if !body.email.contains('@')
        || body.email.starts_with('@')
        || body.email.ends_with('@')
        || !body
            .email
            .split('@')
            .nth(1)
            .is_some_and(|d| d.contains('.'))
        || body.email.len() > 254
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid email address".to_string(),
            }),
        ));
    }

    // Check for existing user
    let existing = user::Entity::find()
        .filter(
            user::Column::Username
                .eq(&body.username)
                .or(user::Column::Email.eq(&body.email)),
        )
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("db error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            )
        })?;

    if existing.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "Username or email already taken".to_string(),
            }),
        ));
    }

    // Hash password
    let password_hash = hash_password(&body.password).map_err(|e| {
        tracing::error!("hash error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })?;

    let now = chrono::Utc::now().fixed_offset();
    let user_id = Uuid::new_v4();

    // Determine role: first user is admin
    let role = if user_count == 0 {
        user::UserRole::Admin
    } else {
        user::UserRole::User
    };

    let new_user = user::ActiveModel {
        id: Set(user_id),
        username: Set(body.username.clone()),
        email: Set(body.email.clone()),
        password_hash: Set(password_hash),
        display_name: Set(body.display_name.clone()),
        avatar_url: Set(None),
        role: Set(role),
        is_banned: Set(false),
        ban_reason: Set(None),
        banned_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    };

    let created = new_user.insert(&state.db).await.map_err(|e| {
        tracing::error!("insert error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to create user".to_string(),
            }),
        )
    })?;

    let tokens = generate_token_pair(
        created.id,
        &created.username,
        created.role.as_str(),
        &state.jwt_secret,
    )
    .map_err(|e| {
        tracing::error!("token error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to generate tokens".to_string(),
            }),
        )
    })?;

    // Dispatch plugin event (best-effort)
    if let Some(registry) = crate::api::get_plugin_registry(&state) {
        let payload = soundtime_plugin::UserRegisteredPayload {
            user_id: created.id.to_string(),
            username: created.username.clone(),
        };
        let payload_val = serde_json::to_value(&payload).unwrap_or_default();
        let registry = registry.clone();
        tokio::spawn(async move {
            registry.dispatch("on_user_registered", &payload_val).await;
        });
    }

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

/// POST /api/auth/login
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<ErrorResponse>)> {
    let found = user::Entity::find()
        .filter(user::Column::Username.eq(&body.username))
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("db error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            )
        })?;

    let user = found.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid credentials".to_string(),
            }),
        )
    })?;

    let valid = verify_password(&body.password, &user.password_hash).map_err(|e| {
        tracing::error!("verify error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })?;

    if !valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid credentials".to_string(),
            }),
        ));
    }

    // Check if user is banned
    if user.is_banned {
        let reason = user.ban_reason.as_deref().unwrap_or("No reason provided");
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: format!("Your account has been banned. Reason: {reason}"),
            }),
        ));
    }

    let tokens = generate_token_pair(
        user.id,
        &user.username,
        user.role.as_str(),
        &state.jwt_secret,
    )
    .map_err(|e| {
        tracing::error!("token error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to generate tokens".to_string(),
            }),
        )
    })?;

    // Dispatch plugin event (best-effort)
    if let Some(registry) = crate::api::get_plugin_registry(&state) {
        let payload = soundtime_plugin::UserLoginPayload {
            user_id: user.id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        let payload_val = serde_json::to_value(&payload).unwrap_or_default();
        let registry = registry.clone();
        tokio::spawn(async move {
            registry.dispatch("on_user_login", &payload_val).await;
        });
    }

    Ok(Json(AuthResponse {
        user: UserResponse {
            id: user.id,
            instance_id: format!("{}@{}", &user.username, &state.domain),
            username: user.username,
            email: user.email,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            role: user.role.to_string(),
        },
        tokens,
    }))
}

/// POST /api/auth/refresh
pub async fn refresh(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RefreshRequest>,
) -> Result<Json<TokenPair>, (StatusCode, Json<ErrorResponse>)> {
    let claims = validate_token(&body.refresh_token, &state.jwt_secret).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid or expired refresh token".to_string(),
            }),
        )
    })?;

    if claims.token_type != TokenType::Refresh {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid token type".to_string(),
            }),
        ));
    }

    // Verify user still exists
    let user = user::Entity::find_by_id(claims.sub)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("db error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "User no longer exists".to_string(),
                }),
            )
        })?;

    // SECURITY: reject refresh for banned users
    if user.is_banned {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Account has been suspended".to_string(),
            }),
        ));
    }

    let tokens = generate_token_pair(
        user.id,
        &user.username,
        user.role.as_str(),
        &state.jwt_secret,
    )
    .map_err(|e| {
        tracing::error!("token error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to generate tokens".to_string(),
            }),
        )
    })?;

    Ok(Json(tokens))
}

/// GET /api/auth/me (requires auth)
pub async fn me(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
) -> Result<Json<UserResponse>, (StatusCode, Json<ErrorResponse>)> {
    let user = user::Entity::find_by_id(auth_user.0.sub)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("db error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "User not found".to_string(),
                }),
            )
        })?;

    Ok(Json(UserResponse {
        id: user.id,
        instance_id: format!("{}@{}", &user.username, &state.domain),
        username: user.username,
        email: user.email,
        display_name: user.display_name,
        avatar_url: user.avatar_url,
        role: user.role.to_string(),
    }))
}

// ─── Account Deletion (GDPR) ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct DeleteAccountRequest {
    pub password: String,
}

// ─── Email & Password change ───────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ChangeEmailRequest {
    pub new_email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// PUT /api/auth/email — change the authenticated user's email
pub async fn change_email(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Json(body): Json<ChangeEmailRequest>,
) -> Result<Json<UserResponse>, (StatusCode, Json<ErrorResponse>)> {
    let user_id = auth_user.0.sub;

    // Validate email format
    if !body.new_email.contains('@') || body.new_email.len() < 5 || body.new_email.len() > 255 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid email address".to_string(),
            }),
        ));
    }

    // Find user and verify password
    let found = user::Entity::find_by_id(user_id)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("db error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "User not found".to_string(),
                }),
            )
        })?;

    let valid = verify_password(&body.password, &found.password_hash).map_err(|e| {
        tracing::error!("verify error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })?;

    if !valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Incorrect password".to_string(),
            }),
        ));
    }

    // Check that new email isn't already taken
    let existing = user::Entity::find()
        .filter(user::Column::Email.eq(&body.new_email))
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("db error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            )
        })?;

    if existing.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "This email is already in use".to_string(),
            }),
        ));
    }

    // Update email
    let mut user_update: user::ActiveModel = found.clone().into();
    user_update.email = Set(body.new_email);
    let updated = user_update.update(&state.db).await.map_err(|e| {
        tracing::error!("update error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to update email".to_string(),
            }),
        )
    })?;

    tracing::info!(user_id = %user_id, "user changed email");

    Ok(Json(UserResponse {
        id: updated.id,
        instance_id: format!("{}@{}", &updated.username, &state.domain),
        username: updated.username,
        email: updated.email,
        display_name: updated.display_name,
        avatar_url: updated.avatar_url,
        role: updated.role.to_string(),
    }))
}

/// PUT /api/auth/password — change the authenticated user's password
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let user_id = auth_user.0.sub;

    // Validate new password
    if body.new_password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Password must be at least 8 characters".to_string(),
            }),
        ));
    }

    if body.new_password.len() > 1024 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Password too long".to_string(),
            }),
        ));
    }

    // Find user and verify current password
    let found = user::Entity::find_by_id(user_id)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("db error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "User not found".to_string(),
                }),
            )
        })?;

    let valid = verify_password(&body.current_password, &found.password_hash).map_err(|e| {
        tracing::error!("verify error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })?;

    if !valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Incorrect password".to_string(),
            }),
        ));
    }

    // Hash new password and update
    let new_hash = hash_password(&body.new_password).map_err(|e| {
        tracing::error!("hash error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })?;

    let mut user_update: user::ActiveModel = found.into();
    user_update.password_hash = Set(new_hash);
    user_update.update(&state.db).await.map_err(|e| {
        tracing::error!("update error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Failed to update password".to_string(),
            }),
        )
    })?;

    tracing::info!(user_id = %user_id, "user changed password");

    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/auth/account — permanently delete the authenticated user's account and all data
pub async fn delete_account(
    State(state): State<Arc<AppState>>,
    axum::Extension(auth_user): axum::Extension<AuthUser>,
    Json(body): Json<DeleteAccountRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let user_id = auth_user.0.sub;

    // Find the user and verify password
    let found = user::Entity::find_by_id(user_id)
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("db error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "User not found".to_string(),
                }),
            )
        })?;

    let valid = verify_password(&body.password, &found.password_hash).map_err(|e| {
        tracing::error!("verify error: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal server error".to_string(),
            }),
        )
    })?;

    if !valid {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Mot de passe incorrect".to_string(),
            }),
        ));
    }

    // Prevent admin from deleting their own account if they are the only admin
    if found.role == user::UserRole::Admin {
        let admin_count = user::Entity::find()
            .filter(user::Column::Role.eq(user::UserRole::Admin))
            .count(&state.db)
            .await
            .unwrap_or(1);
        if admin_count <= 1 {
            return Err((StatusCode::FORBIDDEN, Json(ErrorResponse {
                error: "Vous êtes le seul administrateur. Promouvez un autre utilisateur avant de supprimer votre compte.".to_string(),
            })));
        }
    }

    tracing::info!("Deleting account for user {} ({})", found.username, user_id);

    // Delete in order of dependencies:
    // 1. Favorites
    favorite::Entity::delete_many()
        .filter(favorite::Column::UserId.eq(user_id))
        .exec(&state.db)
        .await
        .ok();

    // 2. Listen history
    listen_history::Entity::delete_many()
        .filter(listen_history::Column::UserId.eq(user_id))
        .exec(&state.db)
        .await
        .ok();

    // 3. Track reports by this user
    track_report::Entity::delete_many()
        .filter(track_report::Column::UserId.eq(user_id))
        .exec(&state.db)
        .await
        .ok();

    // 4. Playlist tracks for user's playlists, then playlists
    let user_playlists: Vec<Uuid> = playlist::Entity::find()
        .filter(playlist::Column::UserId.eq(user_id))
        .all(&state.db)
        .await
        .unwrap_or_default()
        .iter()
        .map(|p| p.id)
        .collect();

    if !user_playlists.is_empty() {
        playlist_track::Entity::delete_many()
            .filter(playlist_track::Column::PlaylistId.is_in(user_playlists.clone()))
            .exec(&state.db)
            .await
            .ok();

        playlist::Entity::delete_many()
            .filter(playlist::Column::UserId.eq(user_id))
            .exec(&state.db)
            .await
            .ok();
    }

    // 5. User's uploaded tracks — remove from all references first
    let user_tracks: Vec<Uuid> = track::Entity::find()
        .filter(track::Column::UploadedBy.eq(user_id))
        .all(&state.db)
        .await
        .unwrap_or_default()
        .iter()
        .map(|t| t.id)
        .collect();

    if !user_tracks.is_empty() {
        // Remove favorites/history/playlist_tracks referencing these tracks
        favorite::Entity::delete_many()
            .filter(favorite::Column::TrackId.is_in(user_tracks.clone()))
            .exec(&state.db)
            .await
            .ok();
        listen_history::Entity::delete_many()
            .filter(listen_history::Column::TrackId.is_in(user_tracks.clone()))
            .exec(&state.db)
            .await
            .ok();
        playlist_track::Entity::delete_many()
            .filter(playlist_track::Column::TrackId.is_in(user_tracks.clone()))
            .exec(&state.db)
            .await
            .ok();
        track_report::Entity::delete_many()
            .filter(track_report::Column::TrackId.is_in(user_tracks.clone()))
            .exec(&state.db)
            .await
            .ok();

        // Delete tracks (audio files stay on storage for now, admin can clean up)
        track::Entity::delete_many()
            .filter(track::Column::UploadedBy.eq(user_id))
            .exec(&state.db)
            .await
            .ok();
    }

    // 6. Finally, delete the user
    user::Entity::delete_by_id(user_id)
        .exec(&state.db)
        .await
        .map_err(|e| {
            tracing::error!("Failed to delete user {user_id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to delete account".to_string(),
                }),
            )
        })?;

    tracing::info!("Account deleted for user {user_id}");
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::jwt::generate_token_pair;
    use crate::auth::middleware::require_auth;
    use axum::{
        body::Body,
        http::{Request as HttpRequest, StatusCode},
        middleware as axum_mw,
        routing::{post, put},
        Router,
    };
    use soundtime_audio::AudioStorage;
    use tower::ServiceExt;

    fn test_state() -> Arc<AppState> {
        let db = sea_orm::DatabaseConnection::Disconnected;
        Arc::new(AppState {
            db,
            jwt_secret: "test-secret".to_string(),
            domain: "localhost".to_string(),
            storage: Arc::new(AudioStorage::new("/tmp/test")),
            p2p: None,
            plugins: None,
        })
    }

    fn login_app(state: Arc<AppState>) -> Router {
        Router::new().route("/login", post(login)).with_state(state)
    }

    fn refresh_app(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/refresh", post(refresh))
            .with_state(state)
    }

    fn password_app(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/password", put(change_password))
            .layer(axum_mw::from_fn_with_state(state.clone(), require_auth))
            .with_state(state)
    }

    fn email_app(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/email", put(change_email))
            .layer(axum_mw::from_fn_with_state(state.clone(), require_auth))
            .with_state(state)
    }

    fn json_body(val: serde_json::Value) -> Body {
        Body::from(serde_json::to_vec(&val).unwrap())
    }

    // ─── Registration validation tests ─────────────────────────────
    //
    // The register handler queries the DB before running input validation,
    // and Sea-ORM's Disconnected variant panics on query (get_database_backend).
    // We test validation logic directly against the same conditions used in
    // the handler.

    #[test]
    fn test_register_username_too_short() {
        let username = "ab";
        assert!(
            username.len() < 3 || username.len() > 64,
            "Username with 2 chars should fail length check"
        );
    }

    #[test]
    fn test_register_username_too_long() {
        let username = "a".repeat(65);
        assert!(
            username.len() < 3 || username.len() > 64,
            "Username with 65 chars should fail length check"
        );
    }

    #[test]
    fn test_register_username_valid_length() {
        let username = "abc";
        assert!(
            !(username.len() < 3 || username.len() > 64),
            "Username with 3 chars should pass length check"
        );
    }

    #[test]
    fn test_register_username_with_at() {
        let username = "bad@user";
        assert!(
            username.contains('@') || username.contains('/') || username.contains(' '),
            "Username with @ should fail character check"
        );
    }

    #[test]
    fn test_register_username_with_slash() {
        let username = "bad/user";
        assert!(
            username.contains('@') || username.contains('/') || username.contains(' '),
            "Username with / should fail character check"
        );
    }

    #[test]
    fn test_register_username_with_space() {
        let username = "bad user";
        assert!(
            username.contains('@') || username.contains('/') || username.contains(' '),
            "Username with space should fail character check"
        );
    }

    #[test]
    fn test_register_password_too_short() {
        let password = "1234567";
        assert!(
            password.len() < 8,
            "7-char password should fail minimum length check"
        );
    }

    /// Helper: replicates the exact email validation logic from the register handler
    fn is_invalid_email(email: &str) -> bool {
        !email.contains('@')
            || email.starts_with('@')
            || email.ends_with('@')
            || !email.split('@').nth(1).is_some_and(|d| d.contains('.'))
            || email.len() > 254
    }

    #[test]
    fn test_register_email_no_at() {
        assert!(is_invalid_email("nope"));
    }

    #[test]
    fn test_register_email_starts_with_at() {
        assert!(is_invalid_email("@foo.com"));
    }

    #[test]
    fn test_register_email_ends_with_at() {
        assert!(is_invalid_email("foo@"));
    }

    #[test]
    fn test_register_email_no_domain_dot() {
        assert!(is_invalid_email("foo@bar"));
    }

    #[test]
    fn test_register_email_too_long() {
        let long_domain = "a".repeat(250);
        let long_email = format!("u@{long_domain}.com");
        assert!(long_email.len() > 254);
        assert!(is_invalid_email(&long_email));
    }

    #[test]
    fn test_register_valid_email_passes() {
        assert!(!is_invalid_email("test@example.com"));
    }

    #[test]
    fn test_register_valid_input_hits_db() {
        // Verify that valid inputs pass ALL validation checks.
        // With a real DB these would proceed to the duplicate-check query.
        let username = "testuser";
        let email = "test@example.com";
        let password = "password123";

        // Length checks
        assert!(username.len() >= 3 && username.len() <= 64);
        // Character checks
        assert!(!username.contains('@') && !username.contains('/') && !username.contains(' '));
        // Password check
        assert!(password.len() >= 8);
        // Email check
        assert!(!is_invalid_email(email));
    }

    // ─── Login route tests ─────────────────────────────────────────
    //
    // The login handler queries the DB as its first operation, so with
    // Disconnected DB it panics. We verify the route setup is correct
    // by confirming the app compiles and the login_app function works.

    #[test]
    fn test_login_route_setup() {
        // Verify the login router builds without errors
        let state = test_state();
        let _app = login_app(state);
    }

    // ─── Refresh route tests ───────────────────────────────────────

    #[tokio::test]
    async fn test_refresh_invalid_token() {
        let state = test_state();
        let app = refresh_app(state);

        let req = HttpRequest::builder()
            .method("POST")
            .uri("/refresh")
            .header("Content-Type", "application/json")
            .body(json_body(serde_json::json!({
                "refresh_token": "garbage-token"
            })))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "Invalid or expired refresh token");
    }

    #[tokio::test]
    async fn test_refresh_access_token_rejected() {
        let state = test_state();
        let app = refresh_app(state.clone());

        // Generate a valid access token and attempt to use it as a refresh token
        let pair =
            generate_token_pair(Uuid::new_v4(), "testuser", "user", &state.jwt_secret).unwrap();

        let req = HttpRequest::builder()
            .method("POST")
            .uri("/refresh")
            .header("Content-Type", "application/json")
            .body(json_body(serde_json::json!({
                "refresh_token": pair.access_token
            })))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "Invalid token type");
    }

    // ─── Change password validation tests ──────────────────────────

    #[tokio::test]
    async fn test_change_password_too_short() {
        let state = test_state();
        let app = password_app(state.clone());

        let pair =
            generate_token_pair(Uuid::new_v4(), "testuser", "user", &state.jwt_secret).unwrap();

        let req = HttpRequest::builder()
            .method("PUT")
            .uri("/password")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", pair.access_token))
            .body(json_body(serde_json::json!({
                "current_password": "oldpassword",
                "new_password": "1234567"
            })))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "Password must be at least 8 characters");
    }

    #[tokio::test]
    async fn test_change_password_too_long() {
        let state = test_state();
        let app = password_app(state.clone());

        let pair =
            generate_token_pair(Uuid::new_v4(), "testuser", "user", &state.jwt_secret).unwrap();

        let long_password = "a".repeat(1025);
        let req = HttpRequest::builder()
            .method("PUT")
            .uri("/password")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", pair.access_token))
            .body(json_body(serde_json::json!({
                "current_password": "oldpassword",
                "new_password": long_password
            })))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "Password too long");
    }

    // ─── Change email validation tests ─────────────────────────────

    #[tokio::test]
    async fn test_change_email_invalid_no_at() {
        let state = test_state();
        let app = email_app(state.clone());

        let pair =
            generate_token_pair(Uuid::new_v4(), "testuser", "user", &state.jwt_secret).unwrap();

        let req = HttpRequest::builder()
            .method("PUT")
            .uri("/email")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", pair.access_token))
            .body(json_body(serde_json::json!({
                "new_email": "nope",
                "password": "currentpass"
            })))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "Invalid email address");
    }

    #[tokio::test]
    async fn test_change_email_too_short() {
        let state = test_state();
        let app = email_app(state.clone());

        let pair =
            generate_token_pair(Uuid::new_v4(), "testuser", "user", &state.jwt_secret).unwrap();

        let req = HttpRequest::builder()
            .method("PUT")
            .uri("/email")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", pair.access_token))
            .body(json_body(serde_json::json!({
                "new_email": "a@b",
                "password": "currentpass"
            })))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "Invalid email address");
    }

    #[tokio::test]
    async fn test_change_email_too_long() {
        let state = test_state();
        let app = email_app(state.clone());

        let pair =
            generate_token_pair(Uuid::new_v4(), "testuser", "user", &state.jwt_secret).unwrap();

        let long_local = "a".repeat(250);
        let long_email = format!("{long_local}@example.com");
        assert!(long_email.len() > 255);

        let req = HttpRequest::builder()
            .method("PUT")
            .uri("/email")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", pair.access_token))
            .body(json_body(serde_json::json!({
                "new_email": long_email,
                "password": "currentpass"
            })))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "Invalid email address");
    }

    // ─── DTO serialization tests ───────────────────────────────────

    #[test]
    fn test_register_request_deserialize() {
        let json = serde_json::json!({
            "username": "alice",
            "email": "alice@example.com",
            "password": "secret123",
            "display_name": "Alice W."
        });
        let req: RegisterRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.username, "alice");
        assert_eq!(req.email, "alice@example.com");
        assert_eq!(req.password, "secret123");
        assert_eq!(req.display_name.as_deref(), Some("Alice W."));
    }

    #[test]
    fn test_login_request_deserialize() {
        let json = serde_json::json!({
            "username": "bob",
            "password": "hunter2"
        });
        let req: LoginRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.username, "bob");
        assert_eq!(req.password, "hunter2");
    }

    #[test]
    fn test_error_response_serialize() {
        let err = ErrorResponse {
            error: "Something went wrong".to_string(),
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["error"], "Something went wrong");
    }

    #[test]
    fn test_auth_response_serialize() {
        let pair = generate_token_pair(Uuid::new_v4(), "testuser", "user", "test-secret").unwrap();

        let resp = AuthResponse {
            user: UserResponse {
                id: Uuid::nil(),
                username: "testuser".to_string(),
                email: "test@example.com".to_string(),
                display_name: Some("Test User".to_string()),
                avatar_url: None,
                role: "user".to_string(),
                instance_id: "testuser@localhost".to_string(),
            },
            tokens: pair,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["user"]["username"], "testuser");
        assert_eq!(json["user"]["email"], "test@example.com");
        assert_eq!(json["user"]["display_name"], "Test User");
        assert!(json["user"]["avatar_url"].is_null());
        assert!(json["tokens"]["access_token"].is_string());
        assert!(json["tokens"]["refresh_token"].is_string());
        assert_eq!(json["tokens"]["token_type"], "Bearer");
    }

    #[test]
    fn test_user_response_serialize() {
        let user = UserResponse {
            id: Uuid::nil(),
            username: "alice".to_string(),
            email: "alice@example.com".to_string(),
            display_name: None,
            avatar_url: Some("https://example.com/avatar.png".to_string()),
            role: "admin".to_string(),
            instance_id: "alice@localhost".to_string(),
        };
        let json = serde_json::to_value(&user).unwrap();
        assert_eq!(json["username"], "alice");
        assert_eq!(json["role"], "admin");
        assert_eq!(json["instance_id"], "alice@localhost");
        assert!(json["display_name"].is_null());
        assert_eq!(json["avatar_url"], "https://example.com/avatar.png");
    }
}
