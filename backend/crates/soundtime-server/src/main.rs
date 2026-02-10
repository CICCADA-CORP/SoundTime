use axum::{
    extract::DefaultBodyLimit,
    http::HeaderValue,
    middleware as axum_middleware,
    routing::{get, post},
    Extension, Json, Router,
};
use sea_orm_migration::MigratorTrait;
use serde::Serialize;
use soundtime_db::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod api;
mod auth;
mod listing_worker;
pub mod metadata_lookup;
mod p2p_logs;
mod storage_worker;

#[derive(Serialize)]
struct ApiStatus {
    status: &'static str,
    version: &'static str,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(p2p_logs::P2pLogLayer::new())
        .init();

    // Database connection
    let db_config = soundtime_db::DatabaseConfig::from_env();
    tracing::info!("connecting to database...");
    let db = soundtime_db::connect(&db_config)
        .await
        .expect("failed to connect to database");

    // Run migrations
    tracing::info!("running database migrations...");
    soundtime_migration::Migrator::up(&db, None)
        .await
        .expect("failed to run migrations");
    tracing::info!("migrations complete");

    // Build application state
    let jwt_secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "dev-secret-change-me-in-production".to_string());

    // SECURITY: warn if JWT secret is the default fallback
    if jwt_secret == "dev-secret-change-me-in-production"
        || jwt_secret == "change-me-to-a-secure-random-string"
    {
        tracing::error!(
            "JWT_SECRET is set to a known default value! \
             This is a critical security vulnerability. \
             Set JWT_SECRET to a strong random string (≥32 chars) in production."
        );
        if std::env::var("SOUNDTIME_ENV").unwrap_or_default() == "production" {
            panic!("Refusing to start: JWT_SECRET must be set to a secure value in production.");
        }
    }
    let domain = std::env::var("SOUNDTIME_DOMAIN").unwrap_or_else(|_| "localhost:8080".to_string());

    tracing::info!("instance domain: {}", domain);

    // Initialize storage backend (S3 or local)
    let storage: Arc<dyn soundtime_audio::StorageBackend> = match std::env::var("STORAGE_BACKEND")
        .unwrap_or_default()
        .as_str()
    {
        "s3" => {
            tracing::info!("initializing S3 storage backend");
            let endpoint = std::env::var("S3_ENDPOINT").ok();
            let region = std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());
            let access_key = std::env::var("S3_ACCESS_KEY")
                .expect("S3_ACCESS_KEY is required when STORAGE_BACKEND=s3");
            let secret_key = std::env::var("S3_SECRET_KEY")
                .expect("S3_SECRET_KEY is required when STORAGE_BACKEND=s3");
            let bucket =
                std::env::var("S3_BUCKET").expect("S3_BUCKET is required when STORAGE_BACKEND=s3");
            let prefix = std::env::var("S3_PREFIX").unwrap_or_default();

            Arc::new(
                soundtime_audio::S3Storage::from_config(
                    endpoint.as_deref(),
                    &region,
                    &access_key,
                    &secret_key,
                    &bucket,
                    &prefix,
                )
                .await
                .expect("failed to initialize S3 storage"),
            )
        }
        _ => {
            tracing::info!("using local filesystem storage backend");
            Arc::new(soundtime_audio::AudioStorage::from_env())
        }
    };

    // Optionally start the P2P node
    let p2p: Option<Arc<dyn std::any::Any + Send + Sync>> = {
        let p2p_enabled = std::env::var("P2P_ENABLED")
            .unwrap_or_default()
            .eq_ignore_ascii_case("true");
        if p2p_enabled {
            tracing::info!("starting P2P node (iroh)...");
            let p2p_config = soundtime_p2p::P2pConfig::from_env();
            match soundtime_p2p::P2pNode::start(p2p_config, db.clone()).await {
                Ok(node) => {
                    tracing::info!(node_id = %node.node_id(), "P2P node started");
                    Some(node as Arc<dyn std::any::Any + Send + Sync>)
                }
                Err(e) => {
                    tracing::error!("failed to start P2P node: {e}");
                    tracing::warn!("continuing without P2P support");
                    None
                }
            }
        } else {
            None
        }
    };

    let state = Arc::new(AppState {
        db,
        jwt_secret,
        domain,
        storage,
        p2p,
    });

    // Spawn the editorial playlist auto-regeneration scheduler
    api::editorial::spawn_editorial_scheduler(state.clone());

    // Spawn the storage integrity / sync worker (runs daily)
    storage_worker::spawn(state.clone());

    // Spawn the listing heartbeat worker (announces to public directory)
    listing_worker::spawn(state.clone());

    // Task tracker for async storage operations (sync, integrity check)
    let storage_task_tracker = storage_worker::new_tracker();

    // Rate limiter for auth endpoints: 10 requests per 60 seconds per IP
    let auth_governor_conf = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(6)
            .burst_size(10)
            .finish()
            .expect("failed to build rate limiter config"),
    );

    // Auth routes (public, rate-limited)
    let auth_public = Router::new()
        .route("/register", post(auth::routes::register))
        .route("/login", post(auth::routes::login))
        .route("/refresh", post(auth::routes::refresh))
        .layer(GovernorLayer::new(auth_governor_conf));

    // Auth routes (protected)
    let auth_protected = Router::new()
        .route("/me", get(auth::routes::me))
        .route("/email", axum::routing::put(auth::routes::change_email))
        .route(
            "/password",
            axum::routing::put(auth::routes::change_password),
        )
        .route(
            "/account",
            axum::routing::delete(auth::routes::delete_account),
        )
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_auth,
        ));

    // Public API routes (no auth required, but gated if instance is private)
    let public_api = Router::new()
        .route("/tos", get(api::reports::get_tos))
        .route("/tracks", get(api::tracks::list_tracks))
        .route("/tracks/popular", get(api::tracks::list_popular_tracks))
        .route("/tracks/{id}", get(api::tracks::get_track))
        .route("/tracks/{id}/credits", get(api::tracks::get_track_credits))
        .route("/tracks/{id}/stream", get(api::audio::stream_track))
        .route("/tracks/{id}/lyrics", get(api::lyrics::get_track_lyrics))
        .route("/media/{*path}", get(api::audio::serve_media))
        .route("/albums", get(api::albums::list_albums))
        .route("/albums/{id}", get(api::albums::get_album))
        .route("/artists", get(api::artists::list_artists))
        .route("/artists/{id}", get(api::artists::get_artist))
        .route("/playlists", get(api::playlists::list_playlists))
        .route("/playlists/{id}", get(api::playlists::get_playlist))
        .route("/libraries", get(api::libraries::list_libraries))
        .route("/libraries/{id}", get(api::libraries::get_library))
        .route("/users/{id}", get(api::users::get_user_profile))
        .route("/search", get(api::search::search))
        .route(
            "/editorial-playlists",
            get(api::editorial::list_editorial_playlists),
        )
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_auth_if_private,
        ));

    // Always-public routes (setup, auth) — never gated by private mode
    let always_public_api = Router::new()
        .route("/setup/status", get(api::setup::setup_status))
        .route("/setup/admin", post(api::setup::setup_admin));

    // Protected API routes (auth required)
    let protected_api = Router::new()
        .merge(
            Router::new()
                .route("/upload", post(api::audio::upload_track))
                .route("/upload/batch", post(api::audio::upload_tracks_batch))
                .route("/albums/{id}/cover", post(api::audio::upload_album_cover))
                .layer(DefaultBodyLimit::max(500 * 1024 * 1024)), // 500 MB for audio uploads
        )
        .route("/tracks/my-uploads", get(api::tracks::my_uploads))
        .route("/playlists", post(api::playlists::create_playlist))
        .route(
            "/playlists/{id}",
            axum::routing::put(api::playlists::update_playlist)
                .delete(api::playlists::delete_playlist),
        )
        .route(
            "/playlists/{id}/tracks",
            post(api::playlists::add_track_to_playlist),
        )
        .route(
            "/playlists/{id}/tracks/{track_id}",
            axum::routing::delete(api::playlists::remove_track_from_playlist),
        )
        .route(
            "/tracks/{id}",
            axum::routing::put(api::tracks::update_track).delete(api::tracks::delete_track),
        )
        .route("/setup/instance", post(api::setup::setup_instance))
        .route("/setup/complete", post(api::setup::setup_complete))
        .route("/favorites", get(api::favorites::list_favorites))
        .route("/favorites/check", get(api::favorites::check_favorites))
        .route(
            "/favorites/{track_id}",
            post(api::favorites::add_favorite).delete(api::favorites::remove_favorite),
        )
        .route(
            "/history",
            get(api::history::list_history).post(api::history::log_listen),
        )
        .route("/tracks/{id}/report", post(api::reports::report_track))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            auth::middleware::require_auth,
        ));

    let api_routes = Router::new()
        .nest("/auth", auth_public.merge(auth_protected))
        .merge(always_public_api)
        .merge(public_api)
        .merge(protected_api)
        // Public node info endpoint (for listing server)
        .route("/nodeinfo", get(api::admin::nodeinfo))
        // Public P2P status endpoint
        .route("/p2p/status", get(api::p2p::p2p_status))
        .route("/p2p/network-graph", get(api::p2p::network_graph))
        .route("/p2p/search", get(api::p2p::network_search))
        .nest(
            "/admin",
            Router::new()
                .route("/stats", get(api::admin::get_stats))
                .route("/settings", get(api::admin::get_settings))
                .route(
                    "/settings/{key}",
                    axum::routing::put(api::admin::update_setting),
                )
                .route(
                    "/blocked-domains",
                    get(api::admin::list_blocked_domains).post(api::admin::block_domain),
                )
                .route(
                    "/blocked-domains/export",
                    get(api::admin::export_blocked_domains),
                )
                .route(
                    "/blocked-domains/import",
                    post(api::admin::import_blocked_domains),
                )
                .route(
                    "/blocked-domains/{id}",
                    axum::routing::delete(api::admin::unblock_domain),
                )
                .route("/instances", get(api::admin::list_instances))
                .route(
                    "/instances/health-check",
                    post(api::admin::check_instances_health),
                )
                .route("/listing/trigger", post(listing_worker::trigger_heartbeat))
                .route("/listing/status", get(listing_worker::listing_status))
                .route("/metadata/status", get(api::admin::metadata_status))
                .route(
                    "/metadata/enrich/{track_id}",
                    post(api::admin::enrich_track_metadata),
                )
                .route(
                    "/metadata/enrich-all",
                    post(api::admin::enrich_all_metadata),
                )
                .route("/remote-tracks", get(api::admin::list_remote_tracks))
                .route("/users", get(api::admin::list_users))
                .route(
                    "/users/{id}/role",
                    axum::routing::put(api::admin::update_user_role),
                )
                .route(
                    "/users/{id}/ban",
                    axum::routing::put(api::admin::ban_user).delete(api::admin::unban_user),
                )
                .route("/editorial/status", get(api::editorial::editorial_status))
                .route(
                    "/editorial/generate",
                    post(api::editorial::generate_editorial_playlists),
                )
                .route("/reports", get(api::reports::list_reports))
                .route("/reports/stats", get(api::reports::report_stats))
                .route(
                    "/reports/{id}",
                    axum::routing::put(api::reports::resolve_report),
                )
                .route("/tracks/browse", get(api::reports::browse_tracks))
                .route(
                    "/tracks/{id}/moderate",
                    axum::routing::delete(api::reports::moderate_track),
                )
                .route(
                    "/tos",
                    axum::routing::put(api::reports::update_tos).delete(api::reports::reset_tos),
                )
                .route("/storage/status", get(api::admin::storage_status))
                .route(
                    "/storage/integrity-check",
                    post(api::admin::run_integrity_check),
                )
                .route("/storage/sync", post(api::admin::run_storage_sync))
                .route("/storage/task-status", get(api::admin::storage_task_status))
                .layer(Extension(storage_task_tracker))
                // P2P admin routes
                .route(
                    "/p2p/peers",
                    get(api::p2p::list_peers).post(api::p2p::add_peer),
                )
                .route(
                    "/p2p/peers/{node_id}",
                    axum::routing::delete(api::p2p::remove_peer),
                )
                .route("/p2p/peers/{node_id}/ping", post(api::p2p::ping_peer))
                // P2P log routes
                .route(
                    "/p2p/logs",
                    get(p2p_logs::get_p2p_logs).delete(p2p_logs::clear_p2p_logs),
                )
                .layer(axum_middleware::from_fn_with_state(
                    state.clone(),
                    auth::middleware::require_admin,
                )),
        );

    // CORS configuration — restrict to configured origins
    let cors = {
        let allowed_origins_str = std::env::var("CORS_ORIGINS").unwrap_or_default();
        if allowed_origins_str.is_empty() {
            // Default: allow same-origin only (no cross-origin)
            tracing::warn!("CORS_ORIGINS not set — defaulting to restrictive CORS. Set CORS_ORIGINS=http://localhost:3000 for dev.");
            let scheme = std::env::var("SOUNDTIME_SCHEME").unwrap_or_else(|_| "https".to_string());
            let origin = format!("{scheme}://{}", state.domain);
            CorsLayer::new()
                .allow_origin(AllowOrigin::exact(
                    HeaderValue::from_str(&origin)
                        .unwrap_or_else(|_| HeaderValue::from_static("https://localhost")),
                ))
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::DELETE,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers(tower_http::cors::Any)
                .expose_headers(tower_http::cors::Any)
        } else {
            let origins: Vec<HeaderValue> = allowed_origins_str
                .split(',')
                .filter_map(|s| HeaderValue::from_str(s.trim()).ok())
                .collect();
            tracing::info!("CORS allowed origins: {:?}", origins);
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::DELETE,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers(tower_http::cors::Any)
                .expose_headers(tower_http::cors::Any)
        }
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        // Well-known nodeinfo alias — used by other instances for health checks
        .route(
            "/.well-known/nodeinfo",
            get(api::admin::nodeinfo),
        )
        .nest("/api", api_routes)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        // Security headers
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=63072000; includeSubDomains; preload"),
        ))
        // Content-Security-Policy
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("content-security-policy"),
            HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https:; media-src 'self' blob:; connect-src 'self'; font-src 'self'; frame-ancestors 'none'"),
        ))
        // Referrer-Policy
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("referrer-policy"),
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        // Permissions-Policy
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"),
        ))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!(%addr, "server started");

    axum::serve(
        tokio::net::TcpListener::bind(addr).await.unwrap(),
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

async fn healthz() -> Json<ApiStatus> {
    Json(ApiStatus {
        status: "ok",
        version: soundtime_p2p::build_version(),
    })
}
