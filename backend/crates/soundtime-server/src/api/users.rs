use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use soundtime_db::entities::{playlist, user};
use soundtime_db::AppState;

#[derive(Debug, Serialize)]
pub struct PublicUserResponse {
    pub id: Uuid,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

impl From<user::Model> for PublicUserResponse {
    fn from(u: user::Model) -> Self {
        Self {
            id: u.id,
            username: u.username,
            display_name: u.display_name,
            avatar_url: u.avatar_url,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct UserProfileResponse {
    #[serde(flatten)]
    pub user: PublicUserResponse,
    pub playlists: Vec<super::playlists::PlaylistResponse>,
}

/// GET /api/users/:id
pub async fn get_user_profile(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<UserProfileResponse>, (StatusCode, String)> {
    let user_model = user::Entity::find_by_id(id)
        .one(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?
        .ok_or((StatusCode::NOT_FOUND, "User not found".to_string()))?;

    let pub_playlists = playlist::Entity::find()
        .filter(playlist::Column::UserId.eq(id))
        .filter(playlist::Column::IsPublic.eq(true))
        .all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {e}")))?;

    Ok(Json(UserProfileResponse {
        user: PublicUserResponse::from(user_model),
        playlists: pub_playlists
            .into_iter()
            .map(super::playlists::PlaylistResponse::from)
            .collect(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use soundtime_db::entities::user::UserRole;

    fn make_user_model() -> user::Model {
        user::Model {
            id: Uuid::new_v4(),
            username: "testuser".into(),
            email: "test@example.com".into(),
            password_hash: "hashed".into(),
            display_name: Some("Test User".into()),
            avatar_url: Some("https://img.example.com/avatar.jpg".into()),
            role: UserRole::User,
            is_banned: false,
            ban_reason: None,
            banned_at: None,
            created_at: Utc::now().fixed_offset(),
            updated_at: Utc::now().fixed_offset(),
        }
    }

    #[test]
    fn test_public_user_response_from_model() {
        let model = make_user_model();
        let id = model.id;
        let resp = PublicUserResponse::from(model);
        assert_eq!(resp.id, id);
        assert_eq!(resp.username, "testuser");
        assert_eq!(resp.display_name.as_deref(), Some("Test User"));
    }

    #[test]
    fn test_public_user_response_serialization() {
        let model = make_user_model();
        let resp = PublicUserResponse::from(model);
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["username"], "testuser");
        assert!(json.get("password_hash").is_none());
        assert!(json.get("email").is_none());
    }

    #[test]
    fn test_public_user_response_no_display_name() {
        let mut model = make_user_model();
        model.display_name = None;
        model.avatar_url = None;
        let resp = PublicUserResponse::from(model);
        assert!(resp.display_name.is_none());
        assert!(resp.avatar_url.is_none());
    }
}
