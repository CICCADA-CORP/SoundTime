use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(concat!(
                include_str!("federation/inventories.sql"),
                "\n",
                include_str!("federation/journal_backfill.sql")
            ))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
            "DROP TRIGGER federation_block_control ON blocked_domains;
             DROP FUNCTION federation_serialize_peer_block(),federation_import_signed(text,uuid,bigint,jsonb,jsonb),federation_seed_events(integer);
             DROP INDEX federation_event_snapshot,federation_pex_consent,federation_followed_catalogs;
             DROP TABLE federation_event_bootstrap,federation_catalog_clients;
             DROP TABLE federation_inventory_pages, federation_inventory_seen,
             federation_inventories, federation_signed_publications;",
            )
            .await?;
        Ok(())
    }
}
