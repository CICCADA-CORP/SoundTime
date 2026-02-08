use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"INSERT INTO instance_settings (id, key, value, updated_at)
               VALUES (gen_random_uuid(), 'setup_complete', 'false', NOW())
               ON CONFLICT (key) DO NOTHING"#,
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DELETE FROM instance_settings WHERE key = 'setup_complete'")
            .await?;
        Ok(())
    }
}
