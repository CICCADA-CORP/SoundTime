use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("federation/inventories.sql"))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
            "DROP FUNCTION federation_import_signed(text,uuid,bigint,jsonb,jsonb),federation_seed_events(integer);
             DROP INDEX federation_event_snapshot;
             DROP TABLE federation_event_bootstrap;
             DROP TABLE federation_inventory_pages, federation_inventory_seen,
             federation_inventories, federation_signed_publications;",
            )
            .await?;
        Ok(())
    }
}
