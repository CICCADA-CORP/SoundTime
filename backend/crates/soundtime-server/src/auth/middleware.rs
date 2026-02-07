use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::json;
use std::sync::Arc;

use super::jwt::{validate_token, Claims, TokenType};
use soundtime_db::AppState;

/// Extension type to access authenticated user claims in handlers
#[derive(Clone, Debug)]
pub struct AuthUser(pub Claims);

/// Middleware: require valid access token
pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => &header[7..],
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Missing or invalid Authorization header" })),
            )
                .into_response();
        }
    };

    match validate_token(token, &state.jwt_secret) {
        Ok(claims) if claims.token_type == TokenType::Access => {
            request.extensions_mut().insert(AuthUser(claims));
            next.run(request).await
        }
        Ok(_) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid token type, access token required" })),
        )
            .into_response(),
        Err(_) => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid or expired token" })),
        )
            .into_response(),
    }
}

/// Middleware: require admin role
pub async fn require_admin(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => &header[7..],
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Missing or invalid Authorization header" })),
            )
                .into_response();
        }
    };

    match validate_token(token, &state.jwt_secret) {
        Ok(claims) if claims.token_type == TokenType::Access && claims.role == "admin" => {
            // SECURITY: verify admin role from DB, not just JWT
            let user_id = claims.sub;
            let db = state.db.clone();
            let is_admin = tokio::spawn(async move {
                soundtime_db::entities::user::Entity::find_by_id(user_id)
                    .one(&db)
                    .await
                    .ok()
                    .flatten()
                    .map(|u| {
                        u.role == soundtime_db::entities::user::UserRole::Admin && !u.is_banned
                    })
                    .unwrap_or(false)
            })
            .await
            .unwrap_or(false);

            if !is_admin {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({ "error": "Admin access required" })),
                )
                    .into_response();
            }

            request.extensions_mut().insert(AuthUser(claims));
            next.run(request).await
        }
        Ok(claims) if claims.role != "admin" => (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Admin access required" })),
        )
            .into_response(),
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid or expired token" })),
        )
            .into_response(),
    }
}

/// Middleware: if instance is private, require valid access token.
/// If the instance is public, allows the request through without auth.
pub async fn require_auth_if_private(
    State(state): State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Response {
    // Check if instance is private
    let is_private = soundtime_db::entities::instance_setting::Entity::find()
        .filter(soundtime_db::entities::instance_setting::Column::Key.eq("instance_private"))
        .one(&state.db)
        .await
        .ok()
        .flatten()
        .map(|s| s.value == "true")
        .unwrap_or(false);

    if !is_private {
        // Public instance — let everyone through, but still attach user if token present
        if let Some(auth_header) = request
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
        {
            if auth_header.starts_with("Bearer ") {
                if let Ok(claims) = validate_token(&auth_header[7..], &state.jwt_secret) {
                    if claims.token_type == TokenType::Access {
                        request.extensions_mut().insert(AuthUser(claims));
                    }
                }
            }
        }
        return next.run(request).await;
    }

    // Private instance — require auth
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => &header[7..],
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "This instance is private. Authentication required." })),
            )
                .into_response();
        }
    };

    match validate_token(token, &state.jwt_secret) {
        Ok(claims) if claims.token_type == TokenType::Access => {
            request.extensions_mut().insert(AuthUser(claims));
            next.run(request).await
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid or expired token" })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::jwt::generate_token_pair;
    use axum::{
        body::Body,
        http::{Request as HttpRequest, StatusCode},
        middleware as axum_mw,
        routing::get,
        Router,
    };
    use soundtime_audio::AudioStorage;
    use tower::ServiceExt;

    fn test_state() -> Arc<AppState> {
        let db = sea_orm::DatabaseConnection::Disconnected;
        Arc::new(AppState {
            db,
            jwt_secret: "test-middleware-secret".to_string(),
            domain: "localhost".to_string(),
            storage: Arc::new(AudioStorage::new("/tmp/test-middleware")),
            p2p: None,
        })
    }

    async fn ok_handler() -> &'static str {
        "OK"
    }

    fn auth_app(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/protected", get(ok_handler))
            .layer(axum_mw::from_fn_with_state(state.clone(), require_auth))
            .with_state(state)
    }

    fn admin_app(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/admin", get(ok_handler))
            .layer(axum_mw::from_fn_with_state(state.clone(), require_admin))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_require_auth_no_header() {
        let state = test_state();
        let app = auth_app(state);

        let req = HttpRequest::builder()
            .uri("/protected")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_require_auth_invalid_token() {
        let state = test_state();
        let app = auth_app(state);

        let req = HttpRequest::builder()
            .uri("/protected")
            .header("Authorization", "Bearer invalid-token")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_require_auth_valid_access_token() {
        let state = test_state();
        let app = auth_app(state.clone());

        let pair = generate_token_pair(
            uuid::Uuid::new_v4(),
            "testuser",
            "user",
            &state.jwt_secret,
        )
        .unwrap();

        let req = HttpRequest::builder()
            .uri("/protected")
            .header("Authorization", format!("Bearer {}", pair.access_token))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_require_auth_refresh_token_rejected() {
        let state = test_state();
        let app = auth_app(state.clone());

        let pair = generate_token_pair(
            uuid::Uuid::new_v4(),
            "testuser",
            "user",
            &state.jwt_secret,
        )
        .unwrap();

        let req = HttpRequest::builder()
            .uri("/protected")
            .header("Authorization", format!("Bearer {}", pair.refresh_token))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_require_auth_no_bearer_prefix() {
        let state = test_state();
        let app = auth_app(state);

        let req = HttpRequest::builder()
            .uri("/protected")
            .header("Authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_require_admin_no_header() {
        let state = test_state();
        let app = admin_app(state);

        let req = HttpRequest::builder()
            .uri("/admin")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_require_admin_with_admin_token() {
        // NOTE: With Disconnected DB, the DB role verification fails gracefully
        // (tokio::spawn catches the panic) → returns FORBIDDEN.
        // In production with a real DB, a valid admin JWT + DB admin role → OK.
        let state = test_state();
        let app = admin_app(state.clone());

        let pair = generate_token_pair(
            uuid::Uuid::new_v4(),
            "admin",
            "admin",
            &state.jwt_secret,
        )
        .unwrap();

        let req = HttpRequest::builder()
            .uri("/admin")
            .header("Authorization", format!("Bearer {}", pair.access_token))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        // Without a real DB, the DB verification fails → FORBIDDEN
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_require_admin_with_user_token_forbidden() {
        let state = test_state();
        let app = admin_app(state.clone());

        let pair = generate_token_pair(
            uuid::Uuid::new_v4(),
            "normaluser",
            "user",
            &state.jwt_secret,
        )
        .unwrap();

        let req = HttpRequest::builder()
            .uri("/admin")
            .header("Authorization", format!("Bearer {}", pair.access_token))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_require_admin_invalid_token() {
        let state = test_state();
        let app = admin_app(state);

        let req = HttpRequest::builder()
            .uri("/admin")
            .header("Authorization", "Bearer garbage")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_require_auth_wrong_secret() {
        let state = test_state();
        let app = auth_app(state);

        // Generate token with a different secret
        let pair = generate_token_pair(
            uuid::Uuid::new_v4(),
            "testuser",
            "user",
            "wrong-secret",
        )
        .unwrap();

        let req = HttpRequest::builder()
            .uri("/protected")
            .header("Authorization", format!("Bearer {}", pair.access_token))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
