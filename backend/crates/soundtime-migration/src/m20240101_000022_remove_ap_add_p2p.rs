use sea_orm_migration::prelude::*;

/// Migration 22: Remove ActivityPub tables and add P2P content_hash to tracks.
///
/// Drops: actors, activities, follows, deliveries tables + actor_type enum,
///        actor_id columns from tracks/artists/albums
/// Adds:  content_hash (VARCHAR 64, nullable) to tracks for BLAKE3 blob hash
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── Drop foreign‐key constraints referencing actors ─────────────
        // Tracks → actors
        db.execute_unprepared("ALTER TABLE tracks DROP CONSTRAINT IF EXISTS fk_tracks_actor_id")
            .await?;
        db.execute_unprepared("ALTER TABLE tracks DROP COLUMN IF EXISTS actor_id")
            .await?;

        // Artists → actors
        db.execute_unprepared("ALTER TABLE artists DROP CONSTRAINT IF EXISTS fk_artists_actor_id")
            .await?;
        db.execute_unprepared("ALTER TABLE artists DROP COLUMN IF EXISTS actor_id")
            .await?;

        // Albums → actors
        db.execute_unprepared("ALTER TABLE albums DROP CONSTRAINT IF EXISTS fk_albums_actor_id")
            .await?;
        db.execute_unprepared("ALTER TABLE albums DROP COLUMN IF EXISTS actor_id")
            .await?;

        // ── Drop AP tables (order matters for foreign keys) ─────────────
        db.execute_unprepared("DROP TABLE IF EXISTS deliveries CASCADE")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS follows CASCADE")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS activities CASCADE")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS actors CASCADE")
            .await?;

        // Drop the actor_type enum
        db.execute_unprepared("DROP TYPE IF EXISTS actor_type")
            .await?;

        // ── Add P2P content hash to tracks ──────────────────────────────
        db.execute_unprepared(
            "ALTER TABLE tracks ADD COLUMN IF NOT EXISTS content_hash VARCHAR(64)",
        )
        .await?;

        // Index for looking up tracks by content hash
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_tracks_content_hash ON tracks (content_hash) WHERE content_hash IS NOT NULL",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Remove content_hash
        db.execute_unprepared("DROP INDEX IF EXISTS idx_tracks_content_hash")
            .await?;
        db.execute_unprepared("ALTER TABLE tracks DROP COLUMN IF EXISTS content_hash")
            .await?;

        // Re-add actor_id columns (nullable, no FK since actors table won't exist)
        db.execute_unprepared("ALTER TABLE tracks ADD COLUMN IF NOT EXISTS actor_id UUID")
            .await?;
        db.execute_unprepared("ALTER TABLE artists ADD COLUMN IF NOT EXISTS actor_id UUID")
            .await?;
        db.execute_unprepared("ALTER TABLE albums ADD COLUMN IF NOT EXISTS actor_id UUID")
            .await?;

        // Note: full AP tables are NOT restored in down() — that would require
        // re-running the original actor/activity/follow/delivery migrations.

        Ok(())
    }
}
