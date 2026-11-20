use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("federation/workers.sql"))
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // Rolling back the executable can leave these additive tables in place.
        // Dropping them would discard the identity, bans, and withdrawal history.
        Err(DbErr::Migration(
            "federation worker generations must be preserved on rollback".into(),
        ))
    }
}
