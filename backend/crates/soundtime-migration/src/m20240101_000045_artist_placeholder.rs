use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // A legacy literal "Unknown Artist" can be a real artist or the old
        // uploader's fallback. Preserve its identity until provenance distinguishes
        // the two. Only wholly blank legacy names unambiguously lack a credit.
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE artists ADD COLUMN is_placeholder boolean NOT NULL DEFAULT false;
             UPDATE artists SET is_placeholder = true
             WHERE name ~ '^[[:space:]]*$'",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE artists DROP COLUMN is_placeholder")
            .await?;
        Ok(())
    }
}
