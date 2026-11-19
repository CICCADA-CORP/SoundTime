//! Short-lived browser authorization for native audio and artwork requests.
use super::middleware::AuthUser;
use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::Utc;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use soundtime_db::AppState;
use std::sync::Arc;
use uuid::Uuid;

const COOKIE: &str = "soundtime_media";
const AUDIENCE: &str = "soundtime-media";
const LIFETIME: i64 = 600;

#[derive(Serialize, Deserialize)]
struct MediaGrant {
    sub: Uuid,
    aud: String,
    exp: i64,
}

fn issue(user: Uuid, exp: i64, secret: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let claims = MediaGrant {
        sub: user,
        aud: AUDIENCE.into(),
        exp,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes())?,
    )
}

pub(super) fn account(headers: &HeaderMap, secret: &str) -> Option<Uuid> {
    let token = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .flat_map(|cookies| cookies.split(';'))
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find_map(|(name, value)| (name == COOKIE).then_some(value));
    let token = token?;
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&[AUDIENCE]);
    validation.leeway = 0;
    let key = DecodingKey::from_secret(secret.as_bytes()).ok()?;
    decode::<MediaGrant>(token, &key, &validation)
        .ok()
        .map(|data| data.claims.sub)
}

fn cookie_headers(request: &HeaderMap, token: &str, lifetime: i64) -> HeaderMap {
    // Origin retains the external scheme when an HTTPS proxy forwards to Nginx over HTTP.
    let secure = request
        .get(header::ORIGIN)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|v| v.starts_with("https://"))
        || request
            .get("x-forwarded-proto")
            .and_then(|h| h.to_str().ok())
            .is_some_and(|v| v.split(',').next() == Some("https"));
    let flag = if secure { "; Secure" } else { "" };
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    for path in ["/api/tracks/", "/api/media/"] {
        let cookie = format!(
            "{COOKIE}={token}; Path={path}; Max-Age={lifetime}; HttpOnly; SameSite=Strict{flag}"
        );
        headers.append(
            header::SET_COOKIE,
            HeaderValue::from_str(&cookie).expect("JWT is valid in a cookie"),
        );
    }
    headers
}

/// The route requires a valid access Bearer before issuing either cookie.
pub async fn establish(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthUser>,
    headers: HeaderMap,
) -> Response {
    let now = Utc::now().timestamp();
    let expires_in = LIFETIME.min(user.0.exp - now);
    // The browser renews 45 seconds early. Request a refreshed access token
    // when the current one can no longer cover that window.
    if expires_in <= 45 {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Refresh access before starting media"})),
        )
            .into_response();
    }
    match issue(user.0.sub, now + expires_in, &state.jwt_secret) {
        Ok(token) => (
            cookie_headers(&headers, &token, expires_in),
            Json(serde_json::json!({"expires_in": expires_in})),
        )
            .into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Allow browser logout to clear expired credentials as well.
pub async fn clear(headers: HeaderMap) -> impl IntoResponse {
    (StatusCode::NO_CONTENT, cookie_headers(&headers, "", 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::jwt::{generate_token_pair, validate_token};

    #[test]
    fn media_grants_and_api_tokens_are_not_interchangeable() {
        let secret = "media-test-signing-key-at-least-32-bytes";
        let id = Uuid::new_v4();
        let media = issue(id, Utc::now().timestamp() + LIFETIME, secret).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("other=1; {COOKIE}={media}").parse().unwrap(),
        );
        assert_eq!(account(&headers, secret), Some(id));
        assert!(account(&headers, "wrong-signing-key").is_none());
        assert!(validate_token(&media, secret).is_err());
        let expired = issue(id, Utc::now().timestamp() - 1, secret).unwrap();
        headers.insert(
            header::COOKIE,
            format!("{COOKIE}={expired}").parse().unwrap(),
        );
        assert!(account(&headers, secret).is_none());
        let pair = generate_token_pair(id, "test", "user", secret).unwrap();
        for token in [pair.access_token, pair.refresh_token] {
            headers.insert(header::COOKIE, format!("{COOKIE}={token}").parse().unwrap());
            assert!(account(&headers, secret).is_none());
        }
    }

    #[test]
    fn scoped_cookies_are_removed_with_the_same_attributes() {
        let mut request = HeaderMap::new();
        request.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://music.test"),
        );
        for lifetime in [LIFETIME, 0] {
            let headers = cookie_headers(&request, "", lifetime);
            let values: Vec<_> = headers
                .get_all(header::SET_COOKIE)
                .iter()
                .map(|h| h.to_str().unwrap())
                .collect();
            assert_eq!(values.len(), 2);
            assert!(values[0].contains("Path=/api/tracks/"));
            assert!(values[1].contains("Path=/api/media/"));
            assert!(values.iter().all(|v| v.contains(&format!(
                "Max-Age={lifetime}; HttpOnly; SameSite=Strict; Secure"
            ))));
        }
    }
}
