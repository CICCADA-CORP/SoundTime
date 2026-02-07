use sea_orm_migration::prelude::*;

use super::m20240101_000002_create_actors::Actors;
use super::m20240101_000003_create_artists::Artists;
use super::m20240101_000004_create_albums::Albums;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Tracks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Tracks::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Tracks::Title).string_len(255).not_null())
                    .col(ColumnDef::new(Tracks::ArtistId).uuid().not_null())
                    .col(ColumnDef::new(Tracks::AlbumId).uuid().null())
                    .col(ColumnDef::new(Tracks::TrackNumber).small_integer().null())
                    .col(ColumnDef::new(Tracks::DiscNumber).small_integer().null().default(1))
                    .col(ColumnDef::new(Tracks::DurationSecs).float().not_null())
                    .col(ColumnDef::new(Tracks::Genre).string_len(128).null())
                    .col(ColumnDef::new(Tracks::Year).small_integer().null())
                    .col(ColumnDef::new(Tracks::MusicbrainzId).string_len(36).null())
                    .col(ColumnDef::new(Tracks::FilePath).string_len(1024).not_null())
                    .col(ColumnDef::new(Tracks::FileSize).big_integer().not_null())
                    .col(ColumnDef::new(Tracks::Format).string_len(16).not_null())
                    .col(ColumnDef::new(Tracks::Bitrate).integer().null())
                    .col(ColumnDef::new(Tracks::SampleRate).integer().null())
                    .col(ColumnDef::new(Tracks::WaveformData).json_binary().null())
                    .col(ColumnDef::new(Tracks::ActorId).uuid().null())
                    .col(
                        ColumnDef::new(Tracks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tracks_artist_id")
                            .from(Tracks::Table, Tracks::ArtistId)
                            .to(Artists::Table, Artists::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tracks_album_id")
                            .from(Tracks::Table, Tracks::AlbumId)
                            .to(Albums::Table, Albums::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tracks_actor_id")
                            .from(Tracks::Table, Tracks::ActorId)
                            .to(Actors::Table, Actors::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tracks_artist_id")
                    .table(Tracks::Table)
                    .col(Tracks::ArtistId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tracks_album_id")
                    .table(Tracks::Table)
                    .col(Tracks::AlbumId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tracks_title")
                    .table(Tracks::Table)
                    .col(Tracks::Title)
                    .to_owned(),
            )
            .await?;

        // Full-text search index on title
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE INDEX idx_tracks_title_fts ON tracks USING gin(to_tsvector('english', title))",
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Tracks::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum Tracks {
    Table,
    Id,
    Title,
    ArtistId,
    AlbumId,
    TrackNumber,
    DiscNumber,
    DurationSecs,
    Genre,
    Year,
    MusicbrainzId,
    FilePath,
    FileSize,
    Format,
    Bitrate,
    SampleRate,
    WaveformData,
    ActorId,
    CreatedAt,
}
