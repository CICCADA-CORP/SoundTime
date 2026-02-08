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
            .map_or(false, |d| d.contains('.'))
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

    tracing::info!("User {} changed email", user_id);

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

    tracing::info!("User {} changed password", user_id);

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
