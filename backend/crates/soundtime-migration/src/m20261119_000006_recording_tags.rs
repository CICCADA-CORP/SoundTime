use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TABLE track_recording_hints (
            track_id uuid PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
            recording_id uuid, isrc text CHECK(isrc ~ '^[A-Z]{2}[A-Z0-9]{3}[0-9]{7}$'),
            CHECK(recording_id IS NOT NULL OR isrc IS NOT NULL))",
            )
            .await?;
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE track_recording_hints")
            .await?;
        Ok(())
    }
}
