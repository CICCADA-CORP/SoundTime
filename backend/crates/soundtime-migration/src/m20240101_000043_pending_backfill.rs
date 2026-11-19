use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(concat!(
            "CREATE INDEX federation_active_catalog_track ON track_publications(catalog_id, track_id) WHERE state='active';\n",
            include_str!("federation/publication_backfill.sql"),
            include_str!("federation/search_backfill.sql"),
            include_str!("federation/journal_backfill.sql"),
        )).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // The revised batch functions implement the same cursor and return contracts.
        manager
            .get_connection()
            .execute_unprepared("DROP INDEX federation_active_catalog_track")
            .await?;
        Ok(())
    }
}
