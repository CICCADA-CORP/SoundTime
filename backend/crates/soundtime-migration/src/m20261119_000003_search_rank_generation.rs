use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared("
            ALTER TABLE federation_search_documents ADD COLUMN rank_xid xid8;
            CREATE FUNCTION federation_stamp_rank() RETURNS trigger LANGUAGE plpgsql AS $$
            BEGIN NEW.rank_xid := pg_current_xact_id(); RETURN NEW; END $$;
            CREATE TRIGGER federation_stamp_rank BEFORE INSERT OR UPDATE OF document, track_id
                ON federation_search_documents FOR EACH ROW EXECUTE FUNCTION federation_stamp_rank();
            CREATE INDEX federation_rank_xid ON federation_search_documents(rank_xid) WHERE rank_xid IS NOT NULL;
        ").await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "
            DROP TRIGGER federation_stamp_rank ON federation_search_documents;
            DROP FUNCTION federation_stamp_rank();
            ALTER TABLE federation_search_documents DROP COLUMN rank_xid;
        ",
            )
            .await?;
        Ok(())
    }
}
