use sea_orm_migration::prelude::*;

use super::m20240101_000005_create_tracks::Tracks;
use super::m20240101_000006_create_libraries::Libraries;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(LibraryTracks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(LibraryTracks::LibraryId).uuid().not_null())
                    .col(ColumnDef::new(LibraryTracks::TrackId).uuid().not_null())
                    .primary_key(
                        Index::create()
                            .col(LibraryTracks::LibraryId)
                            .col(LibraryTracks::TrackId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_library_tracks_library_id")
                            .from(LibraryTracks::Table, LibraryTracks::LibraryId)
                            .to(Libraries::Table, Libraries::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_library_tracks_track_id")
                            .from(LibraryTracks::Table, LibraryTracks::TrackId)
                            .to(Tracks::Table, Tracks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(LibraryTracks::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum LibraryTracks {
    Table,
    LibraryId,
    TrackId,
}
