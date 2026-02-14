use sea_orm_migration::prelude::*;

/// Migration 26: Fix album deduplication for multi-artist compilations.
///
/// - Merges duplicate albums (same title, different artists) by reassigning
///   tracks to the album with the lowest created_at.
/// - Adds a UNIQUE constraint on (title, artist_id) to prevent future duplicates.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── Step 1: Merge duplicate albums ──────────────────────────────
        // For each group of albums sharing the same title and artist_id,
        // keep the one with the earliest created_at and reassign all tracks.
        db.execute_unprepared(
            "
            WITH duplicates AS (
                SELECT id,
                       title,
                       artist_id,
                       ROW_NUMBER() OVER (
                           PARTITION BY title, artist_id
                           ORDER BY created_at ASC
                       ) AS rn
                FROM albums
            ),
            keeper AS (
                SELECT id AS keeper_id, title, artist_id
                FROM duplicates
                WHERE rn = 1
            ),
            to_remove AS (
                SELECT d.id AS dup_id, k.keeper_id
                FROM duplicates d
                JOIN keeper k ON d.title = k.title AND d.artist_id = k.artist_id
                WHERE d.rn > 1
            )
            UPDATE tracks
            SET album_id = tr.keeper_id
            FROM to_remove tr
            WHERE tracks.album_id = tr.dup_id
            ",
        )
        .await?;

        // ── Step 2: Delete orphaned duplicate albums ────────────────────
        db.execute_unprepared(
            "
            DELETE FROM albums
            WHERE id IN (
                SELECT a.id
                FROM albums a
                LEFT JOIN tracks t ON t.album_id = a.id
                WHERE t.id IS NULL
                  AND a.id NOT IN (
                      SELECT DISTINCT album_id FROM tracks WHERE album_id IS NOT NULL
                  )
            )
            ",
        )
        .await?;

        // ── Step 3: Add UNIQUE constraint ───────────────────────────────
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_albums_title_artist ON albums (title, artist_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS uq_albums_title_artist")
            .await?;
        Ok(())
    }
}
