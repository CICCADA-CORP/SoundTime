use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(concat!(
                include_str!("federation/source_updates.sql"),
                include_str!("federation/withdrawn_fingerprints.sql"),
            ))
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Rewinding a cursor seen by a peer could skip an owner withdrawal.
        manager.get_connection().execute_unprepared("
            LOCK TABLE federation_missing_sources, federation_source_events, federation_withdrawn_fingerprints IN ACCESS EXCLUSIVE MODE;
            DO $$ BEGIN
                IF EXISTS (SELECT FROM federation_source_events) OR EXISTS (SELECT FROM federation_withdrawn_fingerprints) THEN
                    RAISE EXCEPTION 'persisted catalogue cursors and fingerprint work must survive rollback';
                END IF;
            END $$;
            DROP TRIGGER federation_source_transition ON federation_missing_sources;
            DROP FUNCTION federation_record_source_transition();
            DROP TABLE federation_source_events;
            DROP TABLE federation_withdrawn_fingerprints;
        ").await?;
        Ok(())
    }
}
