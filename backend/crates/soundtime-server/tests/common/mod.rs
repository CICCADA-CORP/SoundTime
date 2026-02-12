// Shared test utilities for integration tests
use soundtime_audio::AudioStorage;
use soundtime_db::AppState;
use std::sync::Arc;

/// Create a test AppState with a mock database and temporary storage
pub fn test_app_state(db: sea_orm::DatabaseConnection, tmp_dir: &std::path::Path) -> Arc<AppState> {
    Arc::new(AppState {
        db,
        jwt_secret: "test-jwt-secret-for-testing-only".to_string(),
        domain: "test.soundtime.local".to_string(),
        storage: Arc::new(AudioStorage::new(tmp_dir)),
        p2p: None,
        plugins: None,
    })
}

pub const TEST_JWT_SECRET: &str = "test-jwt-secret-for-testing-only";
