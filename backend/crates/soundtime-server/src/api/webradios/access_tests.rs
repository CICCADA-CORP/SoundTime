use super::*;
use crate::auth::{
    jwt::generate_token_pair,
    middleware::{require_auth, require_auth_if_private},
};
use axum::{
    body::{to_bytes, Body},
    http::{Method, Request},
    middleware,
};
use sea_orm::{ConnectOptions, Database};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().uri(uri).method(method);
    if let Some(token) = token {
        req = req.header("authorization", format!("Bearer {token}"));
    }
    let body = if let Some(json) = body {
        req = req.header("content-type", "application/json");
        Body::from(json.to_string())
    } else {
        Body::empty()
    };
    let resp = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 8192).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
#[ignore = "requires migrated PostgreSQL with TEST_DATABASE_URL"]
async fn user_and_instance_can_each_disable_browsing_and_playback() {
    let mut options = ConnectOptions::new(std::env::var("TEST_DATABASE_URL").unwrap());
    options.max_connections(1).min_connections(1);
    let db = Database::connect(options).await.unwrap();
    db.execute_unprepared("CREATE TEMP TABLE instance_settings(key text PRIMARY KEY,value text NOT NULL);
        CREATE TEMP TABLE user_settings(user_id uuid,key text,value text,updated_at timestamptz,PRIMARY KEY(user_id,key))").await.unwrap();
    let state = Arc::new(AppState {
        db,
        jwt_secret: "test-radio-browser-preferences".into(),
        domain: "localhost".into(),
        storage: Arc::new(soundtime_audio::AudioStorage::new(
            "/tmp/radio-preference-test",
        )),
        p2p: None,
        plugins: None,
        #[cfg(feature = "redis")]
        redis: None,
    });
    let app = routes(Arc::new(Directory::new().unwrap()))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_auth_if_private,
        ))
        .merge(
            Router::new()
                .route("/webradios/preference", axum::routing::put(preference))
                .layer(middleware::from_fn_with_state(state.clone(), require_auth)),
        )
        .with_state(state.clone());
    let token = generate_token_pair(Uuid::new_v4(), "listener", "user", &state.jwt_secret)
        .unwrap()
        .access_token;
    let station = "/webradios/stations/00000000-0000-4000-8000-000000000001/play";
    let status = "/webradios/status";
    let pref = "/webradios/preference";
    assert_eq!(
        request(&app, Method::GET, status, None, None).await,
        (
            StatusCode::OK,
            json!({"enabled":true,"instance_enabled":true,"user_enabled":true})
        )
    );
    assert_eq!(
        request(
            &app,
            Method::PUT,
            pref,
            None,
            Some(json!({"enabled":false}))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            Method::PUT,
            pref,
            Some(&token),
            Some(json!({"enabled":false}))
        )
        .await
        .1["enabled"],
        false
    );
    for (method, uri) in [
        (Method::GET, "/webradios/stations"),
        (Method::POST, station),
    ] {
        assert_eq!(
            request(&app, method, uri, Some(&token), None).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(&app, Method::GET, status, None, None).await.1["enabled"],
        true
    );
    state
        .db
        .execute_unprepared("INSERT INTO instance_settings VALUES('webradios_enabled','false')")
        .await
        .unwrap();
    assert_eq!(
        request(
            &app,
            Method::PUT,
            pref,
            Some(&token),
            Some(json!({"enabled":true}))
        )
        .await
        .1,
        json!({"enabled":false,"instance_enabled":false,"user_enabled":true})
    );
    for (method, uri) in [
        (Method::GET, "/webradios/stations"),
        (Method::POST, station),
    ] {
        assert_eq!(
            request(&app, method, uri, None, None).await.0,
            StatusCode::FORBIDDEN
        );
    }
    state
        .db
        .execute_unprepared("INSERT INTO instance_settings VALUES('instance_private','true')")
        .await
        .unwrap();
    assert_eq!(
        request(&app, Method::GET, status, None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&app, Method::GET, status, Some(&token), None)
            .await
            .0,
        StatusCode::OK
    );
}
