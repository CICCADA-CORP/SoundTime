use sea_orm_migration::prelude::*;

/// Migration 23: Create p2p_peers table for persistent peer registry.
///
/// Stores known P2P peers so they survive server restarts.
/// Previously persisted as a JSON file which was vulnerable to
/// corruption and lacked transactional safety.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS p2p_peers (
                node_id   VARCHAR(255) PRIMARY KEY,
                name      VARCHAR(255),
                version   VARCHAR(64),
                track_count BIGINT NOT NULL DEFAULT 0,
                is_online BOOLEAN NOT NULL DEFAULT false,
                last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS p2p_peers")
            .await?;
        Ok(())
    }
}
