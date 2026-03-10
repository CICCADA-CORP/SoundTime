//! Migration 33 — Refresh collation version after pgvector Docker image switch.
//!
//! When users upgrade from `postgres:16` to `pgvector/pgvector:pg16`, the
//! underlying glibc version may differ, causing PostgreSQL to warn about a
//! collation version mismatch on every connection.  This migration:
//!
//! 1. Reindexes tables that have indexes on text/varchar columns so that
//!    index ordering matches the current collation rules.
//! 2. Runs `ALTER DATABASE ... REFRESH COLLATION VERSION` to record the
//!    current OS-provided collation version and suppress the warning.
//!
//! Both operations are idempotent and safe to run even when no mismatch
//! exists.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Reindex tables whose indexes depend on text/varchar collation.
        // REINDEX TABLE is a no-op on indexes that are already consistent.
        let tables = [
            "users",
            "artists",
            "albums",
            "tracks",
            "playlists",
            "instance_settings",
        ];
        for table in tables {
            db.execute_unprepared(&format!("REINDEX TABLE {table}"))
                .await?;
        }

        // Record the current OS collation version in pg_database so
        // PostgreSQL stops emitting the mismatch warning.
        // Uses dynamic SQL because ALTER DATABASE requires the db name.
        db.execute_unprepared(
            "DO $$ \
             BEGIN \
               EXECUTE 'ALTER DATABASE ' || quote_ident(current_database()) \
                       || ' REFRESH COLLATION VERSION'; \
             END $$",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Collation refresh is a metadata-only update that cannot and
        // need not be reverted.
        Ok(())
    }

    // Note: ALTER DATABASE cannot execute inside a transaction block.
    // The is_transactional() method has been removed from MigrationTrait.
}
