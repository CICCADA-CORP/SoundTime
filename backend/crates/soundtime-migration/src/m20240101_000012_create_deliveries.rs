use sea_orm_migration::prelude::*;

use super::m20240101_000011_create_activities::Activities;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TYPE delivery_status AS ENUM ('pending', 'delivered', 'failed')",
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Deliveries::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Deliveries::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Deliveries::ActivityId).uuid().not_null())
                    .col(
                        ColumnDef::new(Deliveries::InboxUrl)
                            .string_len(512)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Deliveries::Status)
                            .custom(Alias::new("delivery_status"))
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(Deliveries::Attempts)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Deliveries::LastAttemptAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Deliveries::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_deliveries_activity_id")
                            .from(Deliveries::Table, Deliveries::ActivityId)
                            .to(Activities::Table, Activities::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_deliveries_status")
                    .table(Deliveries::Table)
                    .col(Deliveries::Status)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Deliveries::Table).to_owned())
            .await?;

        manager
            .get_connection()
            .execute_unprepared("DROP TYPE IF EXISTS delivery_status")
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum Deliveries {
    Table,
    Id,
    ActivityId,
    InboxUrl,
    Status,
    Attempts,
    LastAttemptAt,
    CreatedAt,
}
