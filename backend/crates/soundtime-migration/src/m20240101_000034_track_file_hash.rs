use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("ALTER TABLE tracks ADD COLUMN file_hash VARCHAR(64)")
            .await?;
        db.execute_unprepared(
            "CREATE INDEX idx_tracks_file_hash ON tracks(file_hash) WHERE file_hash IS NOT NULL",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE tracks DROP COLUMN file_hash")
            .await?;
        Ok(())
    }
}
