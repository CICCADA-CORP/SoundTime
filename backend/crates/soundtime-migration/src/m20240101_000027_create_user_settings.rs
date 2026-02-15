use sea_orm_migration::prelude::*;

/// Migration 27: Create `user_settings` table for per-user key/value settings.
///
/// Initially used for Last.fm scrobbling (session key, username, toggle),
/// but designed as a generic key/value store reusable for future user preferences.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "
            CREATE TABLE IF NOT EXISTS user_settings (
                id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                key         VARCHAR(255) NOT NULL,
                value       TEXT NOT NULL,
                updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(user_id, key)
            )
            ",
        )
        .await?;

        db.execute_unprepared("CREATE INDEX idx_user_settings_user_id ON user_settings(user_id)")
            .await?;

        db.execute_unprepared(
            "CREATE INDEX idx_user_settings_user_key ON user_settings(user_id, key)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS user_settings")
            .await?;
        Ok(())
    }
}
