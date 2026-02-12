use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use std::env;
use std::sync::Arc;
use std::time::Duration;

pub mod entities;

/// Re-export for convenience
pub use sea_orm;

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout_secs: u64,
    pub idle_timeout_secs: u64,
}

impl DatabaseConfig {
    pub fn from_env() -> Self {
        let url = env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://soundtime:soundtime@localhost:5432/soundtime".to_string()
        });

        Self {
            url,
            max_connections: env::var("DB_MAX_CONNECTIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100),
            min_connections: env::var("DB_MIN_CONNECTIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            connect_timeout_secs: env::var("DB_CONNECT_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8),
            idle_timeout_secs: env::var("DB_IDLE_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
        }
    }
}

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub jwt_secret: String,
    pub domain: String,
    pub storage: Arc<dyn soundtime_audio::StorageBackend>,
    /// P2P node handle (type-erased to avoid circular dependency).
    /// Downcast to `Arc<soundtime_p2p::P2pNode>` in handlers.
    pub p2p: Option<Arc<dyn std::any::Any + Send + Sync>>,
    /// Plugin registry handle (type-erased to avoid circular dependency).
    /// Downcast to `Arc<soundtime_plugin::PluginRegistry>` in handlers.
    pub plugins: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

/// Connect to the database and return a connection pool
pub async fn connect(config: &DatabaseConfig) -> Result<DatabaseConnection, DbErr> {
    let mut opt = ConnectOptions::new(&config.url);
    opt.max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
        .idle_timeout(Duration::from_secs(config.idle_timeout_secs))
        .sqlx_logging(true)
        .sqlx_logging_level(log::LevelFilter::Debug);

    Database::connect(opt).await
}
