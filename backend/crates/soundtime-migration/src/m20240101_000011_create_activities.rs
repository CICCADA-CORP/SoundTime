use sea_orm_migration::prelude::*;

use super::m20240101_000002_create_actors::Actors;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Activities::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Activities::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Activities::ActorId).uuid().not_null())
                    .col(
                        ColumnDef::new(Activities::ActivityType)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Activities::ObjectType)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Activities::ObjectUri)
                            .string_len(512)
                            .not_null(),
                    )
                    .col(ColumnDef::new(Activities::Payload).json_binary().not_null())
                    .col(
                        ColumnDef::new(Activities::IsLocal)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(Activities::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_activities_actor_id")
                            .from(Activities::Table, Activities::ActorId)
                            .to(Actors::Table, Actors::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_activities_actor_id")
                    .table(Activities::Table)
                    .col(Activities::ActorId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_activities_created_at")
                    .table(Activities::Table)
                    .col(Activities::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Activities::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum Activities {
    Table,
    Id,
    ActorId,
    ActivityType,
    ObjectType,
    ObjectUri,
    Payload,
    IsLocal,
    CreatedAt,
}
