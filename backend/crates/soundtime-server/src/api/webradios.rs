//! Radio Browser is an external directory. Permission checks precede every
//! catalogue and playback lookup, including responses served from the page cache.
mod directory;
mod relay;

use crate::auth::middleware::AuthUser;
use axum::{
    extract::{Extension, Path, Query, State},
    http::{header::CACHE_CONTROL, HeaderValue, StatusCode},
    routing::{get, post},
    Json, Router,
};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use soundtime_db::AppState;
use std::sync::Arc;
use uuid::Uuid;

pub const SETTING: &str = "webradios_enabled";

pub use directory::Directory;
pub use relay::stream_station;

pub fn routes(directory: Arc<Directory>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/webradios/status", get(status))
        .route("/webradios/stations", get(stations))
        .route("/webradios/stations/{id}/play", post(play))
        .layer(Extension(directory))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            HeaderValue::from_static("private, no-store"),
        ))
}

#[derive(Debug, Serialize)]
pub struct Access {
    instance_enabled: bool,
    user_enabled: bool,
    enabled: bool,
}

async fn access(db: &impl ConnectionTrait, user: Option<Uuid>) -> Result<Access, StatusCode> {
    let query = Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT COALESCE((SELECT value='true' FROM instance_settings WHERE key=$1),true) AS instance_enabled,
                COALESCE((SELECT value='true' FROM user_settings WHERE user_id=$2 AND key=$1),true) AS user_enabled",
        [SETTING.into(), user.into()]);
    let row = db
        .query_one(query)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let instance_enabled: bool = row
        .try_get("", "instance_enabled")
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let user_enabled: bool = row
        .try_get("", "user_enabled")
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Access {
        instance_enabled,
        user_enabled,
        enabled: instance_enabled && user_enabled,
    })
}

fn account(user: Option<Extension<AuthUser>>) -> Option<Uuid> {
    user.map(|Extension(AuthUser(claims))| claims.sub)
}

async fn status(
    State(state): State<Arc<AppState>>,
    user: Option<Extension<AuthUser>>,
) -> Result<Json<Access>, StatusCode> {
    access(&state.db, account(user)).await.map(Json)
}

#[derive(Deserialize)]
pub struct Preference {
    enabled: bool,
}

pub async fn preference(
    State(state): State<Arc<AppState>>,
    Extension(AuthUser(claims)): Extension<AuthUser>,
    Json(body): Json<Preference>,
) -> Result<Json<Access>, StatusCode> {
    state
        .db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO user_settings(user_id,key,value) VALUES($1,$2,$3)
         ON CONFLICT(user_id,key) DO UPDATE SET value=EXCLUDED.value,updated_at=now()",
            [
                claims.sub.into(),
                SETTING.into(),
                body.enabled.to_string().into(),
            ],
        ))
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    access(&state.db, Some(claims.sub)).await.map(Json)
}

async fn require_enabled(
    db: &impl ConnectionTrait,
    user: Option<Extension<AuthUser>>,
) -> Result<(), StatusCode> {
    if access(db, account(user)).await?.enabled {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

#[derive(Default, Deserialize)]
struct Search {
    #[serde(default)]
    name: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    offset: u32,
    limit: Option<u32>,
}

async fn stations(
    State(state): State<Arc<AppState>>,
    user: Option<Extension<AuthUser>>,
    Extension(directory): Extension<Arc<directory::Directory>>,
    Query(query): Query<Search>,
) -> Result<Json<directory::Page>, StatusCode> {
    require_enabled(&state.db, user).await?;
    let name = query.name.trim();
    let tag = query.tag.trim();
    let limit = query.limit.unwrap_or(30);
    if name.chars().count() > 160
        || tag.chars().count() > 120
        || !(1..=60).contains(&limit)
        || query.offset.checked_add(limit).is_none()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    directory
        .stations(name, tag, query.offset, limit)
        .await
        .map(Json)
}

#[derive(Serialize)]
struct Playback {
    #[serde(flatten)]
    stream: directory::Stream,
    relay_url: Option<String>,
}

async fn play(
    State(state): State<Arc<AppState>>,
    user: Option<Extension<AuthUser>>,
    Extension(directory): Extension<Arc<directory::Directory>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Playback>, StatusCode> {
    require_enabled(&state.db, user).await?;
    let stream = directory.play(id).await?;
    let relay_url = (!stream.hls && stream.url.starts_with("http://"))
        .then(|| format!("/api/webradios/stations/{id}/stream"));
    Ok(Json(Playback { stream, relay_url }))
}

#[cfg(test)]
mod access_tests;
