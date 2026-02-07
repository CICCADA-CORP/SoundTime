use sea_orm_migration::prelude::*;

use super::m20240101_000002_create_actors::Actors;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE TYPE follow_status AS ENUM ('pending', 'accepted', 'rejected')",
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Follows::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Follows::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Follows::FollowerActorId).uuid().not_null())
                    .col(ColumnDef::new(Follows::FollowedActorId).uuid().not_null())
                    .col(
                        ColumnDef::new(Follows::Status)
                            .custom(Alias::new("follow_status"))
                            .not_null()
                            .default("pending"),
                    )
                    .col(ColumnDef::new(Follows::ActivityUri).string_len(512).null())
                    .col(
                        ColumnDef::new(Follows::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_follows_follower_actor_id")
                            .from(Follows::Table, Follows::FollowerActorId)
                            .to(Actors::Table, Actors::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_follows_followed_actor_id")
                            .from(Follows::Table, Follows::FollowedActorId)
                            .to(Actors::Table, Actors::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Unique constraint: one follow per pair
        manager
            .create_index(
                Index::create()
                    .name("idx_follows_unique_pair")
                    .table(Follows::Table)
                    .col(Follows::FollowerActorId)
                    .col(Follows::FollowedActorId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Follows::Table).to_owned())
            .await?;

        manager
            .get_connection()
            .execute_unprepared("DROP TYPE IF EXISTS follow_status")
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum Follows {
    Table,
    Id,
    FollowerActorId,
    FollowedActorId,
    Status,
    ActivityUri,
    CreatedAt,
}
