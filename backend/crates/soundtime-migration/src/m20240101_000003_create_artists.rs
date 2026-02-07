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
                    .table(Artists::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Artists::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Artists::Name).string_len(255).not_null())
                    .col(ColumnDef::new(Artists::MusicbrainzId).string_len(36).null())
                    .col(ColumnDef::new(Artists::Bio).text().null())
                    .col(ColumnDef::new(Artists::ImageUrl).string_len(512).null())
                    .col(ColumnDef::new(Artists::ActorId).uuid().null())
                    .col(
                        ColumnDef::new(Artists::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_artists_actor_id")
                            .from(Artists::Table, Artists::ActorId)
                            .to(Actors::Table, Actors::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_artists_name")
                    .table(Artists::Table)
                    .col(Artists::Name)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Artists::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum Artists {
    Table,
    Id,
    Name,
    MusicbrainzId,
    Bio,
    ImageUrl,
    ActorId,
    CreatedAt,
}
