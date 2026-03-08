//! Migration 29 — Database indexes for radio and history query performance.

use sea_orm_migration::prelude::*;

/// Migration 29: Add indexes for radio and history queries.
///
/// These indexes target the most frequent query patterns introduced by the
/// radio feature and the listen history endpoints:
///
/// - **`idx_listen_history_user_listened`** — Composite index on
///   `listen_history(user_id, listened_at DESC)`. Covers the common query
///   pattern `WHERE user_id = $1 ORDER BY listened_at DESC` used by both
///   paginated history (`GET /api/history`) and recent-history endpoints.
///   Without this index, PostgreSQL must do a full table scan + sort.
///
/// - **`idx_tracks_genre`** — B-tree index on `tracks.genre`. Speeds up the
///   radio seed algorithms (track-based, genre-based, personal-mix) that
///   filter by genre, as well as genre browsing in the UI.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// Creates both indexes.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Covers: WHERE user_id = $1 ORDER BY listened_at DESC
        // Used by: list_history, list_recent_history, seed_personal_mix
        manager
            .create_index(
                Index::create()
                    .name("idx_listen_history_user_listened")
                    .table(ListenHistory::Table)
                    .col(ListenHistory::UserId)
                    .col((ListenHistory::ListenedAt, IndexOrder::Desc))
                    .to_owned(),
            )
            .await?;

        // Covers: WHERE genre = $1 / WHERE genre IN (...)
        // Used by: seed_track (phase 2), seed_artist (phase 2), seed_genre, seed_personal_mix
        manager
            .create_index(
                Index::create()
                    .name("idx_tracks_genre")
                    .table(Tracks::Table)
                    .col(Tracks::Genre)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    /// Drops both indexes (rollback).
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_listen_history_user_listened")
                    .table(ListenHistory::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_tracks_genre")
                    .table(Tracks::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

/// Sea-query identifiers for the `listen_history` table columns.
#[derive(DeriveIden)]
enum ListenHistory {
    Table,
    UserId,
    ListenedAt,
}

/// Sea-query identifiers for the `tracks` table columns.
#[derive(DeriveIden)]
enum Tracks {
    Table,
    Genre,
}
