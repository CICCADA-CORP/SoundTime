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
    request: Request,
    next: Next,
) -> Response {
    private_access(&state, request, next, false, None).await
}

pub async fn require_media_if_private(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    private_access(&state, request, next, true, None).await
}

pub async fn require_stream_if_private(
    State((state, budget)): State<(Arc<AppState>, Arc<super::stream_budget::StreamBudget>)>,
    request: Request,
    next: Next,
) -> Response {
    private_access(&state, request, next, true, Some(&budget)).await
}

fn authenticated_account(request: &mut Request, secret: &str, media: bool) -> Option<uuid::Uuid> {
    if let Some(user) = request.extensions().get::<AuthUser>() {
        return Some(user.0.sub);
    }
    let claims = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .and_then(|token| validate_token(token, secret).ok())
        .filter(|claims| claims.token_type == TokenType::Access);
    if let Some(claims) = claims {
        let id = claims.sub;
        request.extensions_mut().insert(AuthUser(claims));
        return Some(id);
    }
    // A media grant identifies the listener only on these read-only routes.
    // It never installs a bearer identity or grants ordinary API access.
    if media {
        super::media::account(request.headers(), secret)
    } else {
        None
    }
}

async fn private_access(
    state: &AppState,
    mut request: Request,
    next: Next,
    media: bool,
    budget: Option<&Arc<super::stream_budget::StreamBudget>>,
) -> Response {
    let account = authenticated_account(&mut request, &state.jwt_secret, media);
    if account.is_some() && !media {
        return next.run(request).await;
    }
    let permit = match budget.map(|budget| budget.admit(account)).transpose() {
        Ok(permit) => permit,
        Err(status) => return super::stream_budget::busy(status),
    };

    // A verified listener may open the track in either visibility mode. The
    // audio permit always marks the response private, including error bodies.
    if account.is_some() {
        if let Some(permit) = permit {
            return permit.hold(next.run(request).await);
        }
    }

    let setting = soundtime_db::entities::instance_setting::Entity::find()
        .filter(soundtime_db::entities::instance_setting::Column::Key.eq("instance_private"))
        .one(&state.db)
        .await;
    let is_private = match setting {
        Ok(value) => value.is_some_and(|row| row.value == "true"),
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"Instance settings unavailable"})),
            )
                .into_response()
        }
    };
    if is_private && account.is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"This instance is private. Authentication required."})),
        )
            .into_response();
    }

    let mut response = next.run(request).await;
    if is_private && media {
        response.headers_mut().insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("private, no-store"),
        );
    }
    match permit {
        Some(permit) => permit.hold(response),
        None => response,
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
            plugins: None,
            #[cfg(feature = "redis")]
            redis: None,
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
    async fn stream_admission_authenticates_listeners_before_reading_private_settings() {
        use crate::auth::{media, stream_budget::StreamBudget};
        use axum::http::header;
        let state = test_state();
        let budget = StreamBudget::new(3, 1, 1);
        let uid = uuid::Uuid::new_v4();
        let pair = generate_token_pair(uid, "one", "user", &state.jwt_secret).unwrap();
        let claims = validate_token(&pair.access_token, &state.jwt_secret).unwrap();
        let cookie = media::establish(
            State(state.clone()),
            axum::Extension(AuthUser(claims.clone())),
            Default::default(),
        )
        .await;
        let cookie = cookie
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let _occupied = budget.admit(Some(uid)).unwrap();
        let _anonymous = budget.admit(None).unwrap();
        let app = Router::new()
            .route("/tracks/{id}/stream", get(ok_handler))
            .layer(axum_mw::from_fn_with_state(
                (state.clone(), budget),
                require_stream_if_private,
            ));
        let other =
            generate_token_pair(uuid::Uuid::new_v4(), "two", "user", &state.jwt_secret).unwrap();
        for (header, value, status) in [
            (
                header::COOKIE,
                cookie.clone(),
                StatusCode::TOO_MANY_REQUESTS,
            ),
            (
                header::AUTHORIZATION,
                format!("Bearer {}", pair.access_token),
                StatusCode::TOO_MANY_REQUESTS,
            ),
            (
                header::COOKIE,
                "soundtime_media=forged".into(),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                header::AUTHORIZATION,
                format!("Bearer {}", pair.refresh_token),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ] {
            let req = HttpRequest::builder()
                .uri(format!("/tracks/{}/stream", uuid::Uuid::new_v4()))
                .header(header, value)
                .header("X-Forwarded-For", "127.0.0.1")
                .body(Body::empty())
                .unwrap();
            let rsp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(rsp.status(), status);
            assert_eq!(rsp.headers()[axum::http::header::RETRY_AFTER], "1");
            assert_eq!(
                rsp.headers()[axum::http::header::CACHE_CONTROL],
                "private, no-store"
            );
            let body = axum::body::to_bytes(rsp.into_body(), 1024).await.unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
                json!({"error":"Audio streaming limit reached"})
            );
        }
        // A second verified listener behind this proxy can reach the handler
        // without querying visibility. Its response stays private in either mode.
        let req = HttpRequest::builder()
            .uri("/tracks/private/stream")
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", other.access_token),
            )
            .header("X-Forwarded-For", "127.0.0.1")
            .body(Body::empty())
            .unwrap();
        let rsp = app.oneshot(req).await.unwrap();
        assert_eq!(rsp.status(), StatusCode::OK);
        assert_eq!(rsp.headers()[header::CACHE_CONTROL], "private, no-store");
        assert!(!rsp.headers().contains_key(header::RETRY_AFTER));
        let mut request = HttpRequest::builder()
            .header(header::COOKIE, cookie)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            authenticated_account(&mut request, &state.jwt_secret, true),
            Some(uid)
        );
        assert!(request.extensions().get::<AuthUser>().is_none());
        assert_eq!(
            authenticated_account(&mut request, &state.jwt_secret, false),
            None
        );
        request.extensions_mut().insert(AuthUser(claims));
        assert_eq!(
            authenticated_account(&mut request, &state.jwt_secret, true),
            Some(uid)
        );
    }

    #[tokio::test]
    async fn authenticated_metadata_needs_no_visibility_query() {
        let state = test_state(); // no database connection
        let uid = uuid::Uuid::new_v4();
        let pair = generate_token_pair(uid, "listener", "user", &state.jwt_secret).unwrap();
        let app = Router::new()
            .route(
                "/metadata",
                get(
                    move |axum::Extension(user): axum::Extension<AuthUser>| async move {
                        assert_eq!(user.0.sub, uid);
                        StatusCode::OK
                    },
                ),
            )
            .layer(axum_mw::from_fn_with_state(
                state.clone(),
                require_auth_if_private,
            ))
            .with_state(state);
        for (token, expected) in [
            (pair.access_token, StatusCode::OK),
            (pair.refresh_token, StatusCode::SERVICE_UNAVAILABLE),
        ] {
            let req = HttpRequest::builder()
                .uri("/metadata")
                .header("Authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap();
            assert_eq!(app.clone().oneshot(req).await.unwrap().status(), expected);
        }
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

        let pair = generate_token_pair(uuid::Uuid::new_v4(), "testuser", "user", &state.jwt_secret)
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

        let pair = generate_token_pair(uuid::Uuid::new_v4(), "testuser", "user", &state.jwt_secret)
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

        let pair =
            generate_token_pair(uuid::Uuid::new_v4(), "admin", "admin", &state.jwt_secret).unwrap();

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
        let pair =
            generate_token_pair(uuid::Uuid::new_v4(), "testuser", "user", "wrong-secret").unwrap();

        let req = HttpRequest::builder()
            .uri("/protected")
            .header("Authorization", format!("Bearer {}", pair.access_token))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
