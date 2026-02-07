use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // remote_tracks — tracks discovered via federation or metadata lookup
        manager
            .create_table(
                Table::create()
                    .table(RemoteTrack::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(RemoteTrack::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(RemoteTrack::LocalTrackId).uuid())
                    .col(ColumnDef::new(RemoteTrack::MusicbrainzId).string())
                    .col(ColumnDef::new(RemoteTrack::Title).string().not_null())
                    .col(ColumnDef::new(RemoteTrack::ArtistName).string().not_null())
                    .col(ColumnDef::new(RemoteTrack::AlbumTitle).string())
                    .col(ColumnDef::new(RemoteTrack::InstanceDomain).string().not_null())
                    .col(ColumnDef::new(RemoteTrack::RemoteUri).string().not_null().unique_key())
                    .col(ColumnDef::new(RemoteTrack::RemoteStreamUrl).string().not_null())
                    .col(ColumnDef::new(RemoteTrack::Bitrate).integer())
                    .col(ColumnDef::new(RemoteTrack::SampleRate).integer())
                    .col(ColumnDef::new(RemoteTrack::Format).string())
                    .col(ColumnDef::new(RemoteTrack::IsAvailable).boolean().not_null().default(true))
                    .col(ColumnDef::new(RemoteTrack::LastCheckedAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(RemoteTrack::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(RemoteTrack::Table, RemoteTrack::LocalTrackId)
                            .to(Tracks::Table, Tracks::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await?;

        // Index for matching tracks by musicbrainz_id
        manager
            .create_index(
                Index::create()
                    .name("idx_remote_tracks_musicbrainz_id")
                    .table(RemoteTrack::Table)
                    .col(RemoteTrack::MusicbrainzId)
                    .to_owned(),
            )
            .await?;

        // Index for matching by local_track_id
        manager
            .create_index(
                Index::create()
                    .name("idx_remote_tracks_local_track_id")
                    .table(RemoteTrack::Table)
                    .col(RemoteTrack::LocalTrackId)
                    .to_owned(),
            )
            .await?;

        // Index for instance availability checks
        manager
            .create_index(
                Index::create()
                    .name("idx_remote_tracks_instance_domain")
                    .table(RemoteTrack::Table)
                    .col(RemoteTrack::InstanceDomain)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(RemoteTrack::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum RemoteTrack {
    #[iden = "remote_tracks"]
    Table,
    Id,
    LocalTrackId,
    MusicbrainzId,
    Title,
    ArtistName,
    AlbumTitle,
    InstanceDomain,
    RemoteUri,
    RemoteStreamUrl,
    Bitrate,
    SampleRate,
    Format,
    IsAvailable,
    LastCheckedAt,
    CreatedAt,
}

#[derive(Iden)]
enum Tracks {
    Table,
    Id,
}
