use sea_orm_migration::prelude::*;

use super::m20240101_000001_create_users::Users;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("CREATE TYPE actor_type AS ENUM ('person', 'service', 'application')")
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Actors::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Actors::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Actors::UserId).uuid().null())
                    .col(
                        ColumnDef::new(Actors::ActorUri)
                            .string_len(512)
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Actors::InboxUrl).string_len(512).not_null())
                    .col(ColumnDef::new(Actors::OutboxUrl).string_len(512).not_null())
                    .col(ColumnDef::new(Actors::PublicKey).text().not_null())
                    .col(ColumnDef::new(Actors::PrivateKey).text().null())
                    .col(ColumnDef::new(Actors::Domain).string_len(255).not_null())
                    .col(
                        ColumnDef::new(Actors::ActorType)
                            .custom(Alias::new("actor_type"))
                            .not_null()
                            .default("person"),
                    )
                    .col(
                        ColumnDef::new(Actors::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_actors_user_id")
                            .from(Actors::Table, Actors::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_actors_user_id")
                    .table(Actors::Table)
                    .col(Actors::UserId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_actors_domain")
                    .table(Actors::Table)
                    .col(Actors::Domain)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Actors::Table).to_owned())
            .await?;

        manager
            .get_connection()
            .execute_unprepared("DROP TYPE IF EXISTS actor_type")
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum Actors {
    Table,
    Id,
    UserId,
    ActorUri,
    InboxUrl,
    OutboxUrl,
    PublicKey,
    PrivateKey,
    Domain,
    ActorType,
    CreatedAt,
}
