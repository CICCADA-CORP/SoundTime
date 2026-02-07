use sea_orm_migration::prelude::*;

use super::m20240101_000005_create_tracks::Tracks;
use super::m20240101_000008_create_playlists::Playlists;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PlaylistTracks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(PlaylistTracks::PlaylistId).uuid().not_null())
                    .col(ColumnDef::new(PlaylistTracks::TrackId).uuid().not_null())
                    .col(
                        ColumnDef::new(PlaylistTracks::Position)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .primary_key(
                        Index::create()
                            .col(PlaylistTracks::PlaylistId)
                            .col(PlaylistTracks::TrackId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_playlist_tracks_playlist_id")
                            .from(PlaylistTracks::Table, PlaylistTracks::PlaylistId)
                            .to(Playlists::Table, Playlists::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_playlist_tracks_track_id")
                            .from(PlaylistTracks::Table, PlaylistTracks::TrackId)
                            .to(Tracks::Table, Tracks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(PlaylistTracks::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum PlaylistTracks {
    Table,
    PlaylistId,
    TrackId,
    Position,
}
