use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(concat!(
                include_str!("federation/search.sql"),
                "\n",
                include_str!("federation/search_backfill.sql")
            ))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(
            "DROP FUNCTION federation_track_availability(boolean,boolean);
             DROP VIEW federation_track_providers, federation_legacy_denied_hashes, federation_search_input;
             DROP TRIGGER federation_search_artist ON artists;
             DROP TRIGGER federation_search_album ON albums;
             DROP TRIGGER federation_search_track_insert ON tracks;
             DROP TRIGGER federation_search_track_update ON tracks;
             DROP FUNCTION federation_refresh_search_batch(integer), federation_search_renamed(),
               federation_search_track_changed(), federation_search_project(tracks,boolean);
             DROP TABLE federation_search_jobs, federation_search_documents;
             DROP FUNCTION federation_search_vector(text,text,text);"
        ).await?;
        Ok(())
    }
}
