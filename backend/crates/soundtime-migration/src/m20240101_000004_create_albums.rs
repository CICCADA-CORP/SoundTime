use sea_orm_migration::prelude::*;

use super::m20240101_000002_create_actors::Actors;
use super::m20240101_000003_create_artists::Artists;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Albums::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Albums::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Albums::Title).string_len(255).not_null())
                    .col(ColumnDef::new(Albums::ArtistId).uuid().not_null())
                    .col(ColumnDef::new(Albums::ReleaseDate).date().null())
                    .col(ColumnDef::new(Albums::CoverUrl).string_len(512).null())
                    .col(ColumnDef::new(Albums::MusicbrainzId).string_len(36).null())
                    .col(ColumnDef::new(Albums::Genre).string_len(128).null())
                    .col(ColumnDef::new(Albums::Year).small_integer().null())
                    .col(ColumnDef::new(Albums::ActorId).uuid().null())
                    .col(
                        ColumnDef::new(Albums::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_albums_artist_id")
                            .from(Albums::Table, Albums::ArtistId)
                            .to(Artists::Table, Artists::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_albums_actor_id")
                            .from(Albums::Table, Albums::ActorId)
                            .to(Actors::Table, Actors::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_albums_artist_id")
                    .table(Albums::Table)
                    .col(Albums::ArtistId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_albums_title")
                    .table(Albums::Table)
                    .col(Albums::Title)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Albums::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum Albums {
    Table,
    Id,
    Title,
    ArtistId,
    ReleaseDate,
    CoverUrl,
    MusicbrainzId,
    Genre,
    Year,
    ActorId,
    CreatedAt,
}
