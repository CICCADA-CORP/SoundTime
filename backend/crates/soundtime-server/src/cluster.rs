//! Split PostgreSQL catalogue reads from the single owner of an instance identity.
use axum::{middleware, routing::get, Router};
use sea_orm::{
    ConnAcquireErr, ConnectionTrait, DatabaseConnection, DbBackend, DbErr, RuntimeErr, Statement,
};
use soundtime_db::{AppState, DatabaseConfig};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;

mod lease;
use lease::LeaseSchedule;

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
    let (generation, mut lease) = loop {
        let sent = Instant::now();
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
            break (generation, LeaseSchedule::confirmed(sent));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    // A slow claim or pool handshake has already spent part of the first lease.
    if Instant::now() >= lease.stop_at {
        stop_owner(
            holder,
            generation,
            "initial lease confirmation arrived after its safety deadline",
        );
    }
    let connect = soundtime_db::connect_named(config, Some(format!("st/{generation}/{holder}")));
    let active = match tokio::time::timeout_at(lease.stop_at.into(), connect).await {
        Ok(Ok(pool)) if Instant::now() < lease.stop_at => pool,
        Ok(Err(error)) => stop_owner(holder, generation, error),
        _ => stop_owner(
            holder,
            generation,
            "initial lease elapsed before worker pool was ready",
        ),
    };
    // Keep a dedicated pool for renewals, independent of listener load. A stuck
    // or replaced owner stops before the lease can be granted to another process.
    tokio::spawn(async move {
        loop {
            tokio::time::sleep_until(lease.next_attempt.into()).await;
            let sent = Instant::now();
            let Some(until) = lease.attempt_deadline(sent) else {
                stop_owner(
                    holder,
                    generation,
                    "last confirmed lease reached its safety deadline",
                );
            };
            let renew = db.execute(Statement::from_sql_and_values(DbBackend::Postgres,
                "UPDATE federation_worker_lease SET expires_at=clock_timestamp()+interval '15 seconds'
                 WHERE singleton AND holder=$1 AND generation=$2 AND expires_at>clock_timestamp()",
                [holder.into(), generation.into()]));
            match tokio::time::timeout_at(until.into(), renew).await {
                Ok(Ok(update)) => {
                    if let Err(reason) =
                        lease.acknowledge(update.rows_affected(), sent, Instant::now())
                    {
                        stop_owner(holder, generation, reason);
                    }
                }
                Ok(Err(error)) if transient_renewal(&error) => {
                    tracing::warn!(%holder, generation, %error, "retrying federation lease renewal");
                    lease.retry(Instant::now());
                }
                Ok(Err(error)) => stop_owner(holder, generation, error),
                Err(_) => {
                    tracing::warn!(%holder, generation, "federation lease renewal timed out");
                    lease.retry(Instant::now());
                }
            }
            if Instant::now() >= lease.stop_at {
                stop_owner(
                    holder,
                    generation,
                    "last confirmed lease reached its safety deadline",
                );
            }
        }
    });
    tracing::info!(%holder, generation, "acquired federation owner lease");
    active
}

fn transient_renewal(error: &DbErr) -> bool {
    use sea_orm::sqlx::Error;
    let runtime = match error {
        DbErr::ConnectionAcquire(ConnAcquireErr::Timeout) => return true,
        DbErr::Conn(e) | DbErr::Exec(e) | DbErr::Query(e) => e,
        _ => return false,
    };
    match runtime {
        RuntimeErr::SqlxError(Error::Io(_) | Error::PoolTimedOut) => true,
        RuntimeErr::SqlxError(Error::Database(db)) => db.code().is_some_and(|code| {
            code.starts_with("08") || matches!(code.as_ref(), "53300" | "55P03" | "57014" | "57P03")
        }),
        _ => false,
    }
}

fn stop_owner(holder: Uuid, generation: i64, reason: impl std::fmt::Display) -> ! {
    tracing::error!(%holder, generation, %reason, "lost federation owner lease");
    std::process::exit(75)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_transient_database_failures_are_retried() {
        let reset = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
        assert!(transient_renewal(&DbErr::Exec(RuntimeErr::SqlxError(
            sea_orm::sqlx::Error::Io(reset),
        ))));
        assert!(transient_renewal(&DbErr::ConnectionAcquire(
            ConnAcquireErr::Timeout
        )));
        assert!(!transient_renewal(&DbErr::ConnectionAcquire(
            ConnAcquireErr::ConnectionClosed
        )));
        assert!(!transient_renewal(&DbErr::Exec(RuntimeErr::Internal(
            "invalid renewal SQL".into()
        ))));
    }
}
