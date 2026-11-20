use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE tracks ADD COLUMN cover_url text;
                 CREATE TABLE legacy_single_albums (
                     album_id uuid PRIMARY KEY REFERENCES albums(id) ON DELETE CASCADE
                 );
                 INSERT INTO legacy_single_albums (album_id)
                 SELECT id FROM albums WHERE title='Singles'",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "DROP TABLE legacy_single_albums; ALTER TABLE tracks DROP COLUMN cover_url",
            )
            .await?;
        Ok(())
    }
}
