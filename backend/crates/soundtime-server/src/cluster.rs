//! Split PostgreSQL catalogue reads from the single owner of an instance identity.
use axum::{middleware, routing::get, Router};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use soundtime_db::{AppState, DatabaseConfig};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Standalone,
    Worker,
    Catalogue,
}

impl Role {
    pub fn configured() -> Self {
        match std::env::var("SOUNDTIME_ROLE")
            .as_deref()
            .unwrap_or("standalone")
        {
            "standalone" => Self::Standalone,
            "worker" => Self::Worker,
            "catalog-http" => Self::Catalogue,
            _ => panic!("SOUNDTIME_ROLE must be standalone, worker or catalog-http"),
        }
    }
}

pub async fn has_lease(db: &DatabaseConnection) -> bool {
    db.query_one(Statement::from_string(
        DbBackend::Postgres,
        "SELECT EXISTS(SELECT FROM federation_worker_lease) AS fenced",
    ))
    .await
    .expect("read federation worker generation")
    .expect("missing lease query result")
    .try_get("", "fenced")
    .expect("invalid lease flag")
}

/// Standbys acquire a new generation before opening the identity or touching its cache.
pub async fn claim(db: DatabaseConnection, config: &DatabaseConfig) -> DatabaseConnection {
    let holder = Uuid::new_v4();
    let generation = loop {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "SELECT federation_claim_worker($1) AS generation",
                [holder.into()],
            ))
            .await
            .expect("claim federation worker")
            .expect("claim returned no row");
        if let Some(generation) = row
            .try_get::<Option<i64>>("", "generation")
            .expect("invalid generation")
        {
            break generation;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    let active = soundtime_db::connect_named(config, Some(format!("st/{generation}/{holder}")))
        .await
        .expect("open fenced worker pool");
    // Keep a dedicated pool for renewals, independent of listener load. A stuck
    // or replaced owner stops before the lease can be granted to another process.
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let renew = db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
                "UPDATE federation_worker_lease SET expires_at=clock_timestamp()+interval '15 seconds'
                 WHERE singleton AND holder=$1 AND generation=$2 AND expires_at>clock_timestamp()",
                [holder.into(), generation.into()]));
            match tokio::time::timeout(Duration::from_secs(5), renew).await {
                Ok(Ok(update)) if update.rows_affected() == 1 => {}
                _ => {
                    tracing::error!(%holder, generation, "lost federation owner lease");
                    std::process::exit(75);
                }
            }
        }
    });
    tracing::info!(%holder, generation, "acquired federation owner lease");
    active
}

pub async fn serve_catalogue(db: DatabaseConnection, domain: String, jwt_secret: String) {
    // These handlers select metadata only; all mutations and byte transfers go
    // through the owner, whose budgets apply across all public reverse proxies.
    let state = Arc::new(AppState {
        db,
        jwt_secret,
        domain,
        p2p: None,
        plugins: None,
        #[cfg(feature = "redis")]
        redis: None,
        storage: Arc::new(soundtime_audio::AudioStorage::from_env()),
    });
    let api = Router::new()
        .route("/api/search", get(crate::api::search::search))
        .route("/api/tracks", get(crate::api::tracks::list_tracks))
        .route("/api/albums", get(crate::api::albums::list_albums))
        .route("/api/artists", get(crate::api::artists::list_artists))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::middleware::require_auth_if_private,
        ))
        .route("/healthz", get(crate::healthz))
        .with_state(state);
    crate::serve(api).await;
}
