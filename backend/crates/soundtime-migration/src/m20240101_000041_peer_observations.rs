use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("federation/legacy_copies.sql"))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "DROP VIEW federation_track_providers;
             ALTER VIEW federation_versioned_track_providers RENAME TO federation_track_providers;
             DROP TABLE federation_legacy_copies;
             ALTER TABLE p2p_peers DROP COLUMN presence_checked_at",
            )
            .await?;
        Ok(())
    }
}
