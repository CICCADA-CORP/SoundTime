use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("federation/mirrors.sql"))
            .await?;
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(
            "DROP VIEW federation_track_providers;
             ALTER VIEW federation_origin_track_providers RENAME TO federation_track_providers;
             ALTER TABLE federation_peer_policies DROP COLUMN mirror_cursor, DROP COLUMN source_disabled;
             DROP TABLE federation_mirror_leases,federation_mirror_clock,federation_cached_publications;").await?;
        Ok(())
    }
}
