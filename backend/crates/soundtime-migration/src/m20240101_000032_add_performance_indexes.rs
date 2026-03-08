//! Migration 32 — Performance indexes for metadata enrichment, favorites, and admin queries.

use sea_orm_migration::prelude::*;

/// Migration 32: Add performance indexes.
///
/// These indexes address specific query bottlenecks identified in production:
///
/// - **`idx_tracks_musicbrainz_id`** — Partial B-tree on `tracks.musicbrainz_id`
///   where the value is NOT NULL. Speeds up uniqueness checks during metadata
///   enrichment.
///
/// - **`idx_tracks_musicbrainz_id_null`** — Partial B-tree on `tracks.id`
///   where `musicbrainz_id IS NULL`. Directly supports the enrichment query
///   `WHERE musicbrainz_id IS NULL` which previously did a sequential scan
///   on 105K rows.
///
/// - **`idx_favorites_user_created`** — Composite index on
///   `favorites(user_id, created_at DESC)`. Covers the paginated favorites
///   query `WHERE user_id = $1 ORDER BY created_at DESC`.
///
/// - **`idx_tracks_bitrate_notnull`** — Partial index on `tracks.bitrate`
///   where bitrate IS NOT NULL. Speeds up admin metadata status COUNT queries.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Partial index for enriched tracks (WHERE musicbrainz_id IS NOT NULL)
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_tracks_musicbrainz_id ON tracks (musicbrainz_id) WHERE musicbrainz_id IS NOT NULL"
        ).await?;

        // Partial index for un-enriched tracks (WHERE musicbrainz_id IS NULL)
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_tracks_musicbrainz_id_null ON tracks (id) WHERE musicbrainz_id IS NULL"
        ).await?;

        // Composite index for paginated favorites
        manager
            .create_index(
                Index::create()
                    .name("idx_favorites_user_created")
                    .table(Favorites::Table)
                    .col(Favorites::UserId)
                    .col((Favorites::CreatedAt, IndexOrder::Desc))
                    .to_owned(),
            )
            .await?;

        // Partial index for bitrate status queries
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_tracks_bitrate_notnull ON tracks (bitrate) WHERE bitrate IS NOT NULL"
        ).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP INDEX IF EXISTS idx_tracks_musicbrainz_id").await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_tracks_musicbrainz_id_null").await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_favorites_user_created")
                    .table(Favorites::Table)
                    .to_owned(),
            )
            .await?;

        db.execute_unprepared("DROP INDEX IF EXISTS idx_tracks_bitrate_notnull").await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Favorites {
    Table,
    UserId,
    CreatedAt,
}
