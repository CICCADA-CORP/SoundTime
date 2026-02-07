use sea_orm_migration::prelude::*;

use super::m20240101_000001_create_users::Users;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(InstanceSettings::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(InstanceSettings::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(InstanceSettings::Key).string_len(255).not_null().unique_key())
                    .col(ColumnDef::new(InstanceSettings::Value).text().not_null())
                    .col(
                        ColumnDef::new(InstanceSettings::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(BlockedDomains::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(BlockedDomains::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(BlockedDomains::Domain).string_len(255).not_null().unique_key())
                    .col(ColumnDef::new(BlockedDomains::Reason).text().null())
                    .col(ColumnDef::new(BlockedDomains::BlockedBy).uuid().null())
                    .col(
                        ColumnDef::new(BlockedDomains::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_blocked_domains_blocked_by")
                            .from(BlockedDomains::Table, BlockedDomains::BlockedBy)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        // Seed default federation settings
        manager
            .get_connection()
            .execute_unprepared(
                "INSERT INTO instance_settings (id, key, value) VALUES
                (gen_random_uuid(), 'federation_enabled', 'true'),
                (gen_random_uuid(), 'instance_name', 'SoundTime'),
                (gen_random_uuid(), 'instance_description', 'A federated music streaming instance'),
                (gen_random_uuid(), 'open_registrations', 'true'),
                (gen_random_uuid(), 'auto_accept_follows', 'true'),
                (gen_random_uuid(), 'max_upload_size_mb', '500')
                ON CONFLICT (key) DO NOTHING",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(BlockedDomains::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(InstanceSettings::Table).to_owned()).await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum InstanceSettings {
    Table,
    Id,
    Key,
    Value,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum BlockedDomains {
    Table,
    Id,
    Domain,
    Reason,
    BlockedBy,
    CreatedAt,
}
