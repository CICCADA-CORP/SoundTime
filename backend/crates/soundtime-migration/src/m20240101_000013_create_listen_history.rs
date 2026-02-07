use sea_orm_migration::prelude::*;

use super::m20240101_000001_create_users::Users;
use super::m20240101_000005_create_tracks::Tracks;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ListenHistory::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ListenHistory::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ListenHistory::UserId).uuid().not_null())
                    .col(ColumnDef::new(ListenHistory::TrackId).uuid().not_null())
                    .col(
                        ColumnDef::new(ListenHistory::ListenedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(ListenHistory::DurationListened)
                            .float()
                            .not_null()
                            .default(0.0),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_listen_history_user_id")
                            .from(ListenHistory::Table, ListenHistory::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_listen_history_track_id")
                            .from(ListenHistory::Table, ListenHistory::TrackId)
                            .to(Tracks::Table, Tracks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_listen_history_user_id")
                    .table(ListenHistory::Table)
                    .col(ListenHistory::UserId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_listen_history_listened_at")
                    .table(ListenHistory::Table)
                    .col(ListenHistory::ListenedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ListenHistory::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum ListenHistory {
    Table,
    Id,
    UserId,
    TrackId,
    ListenedAt,
    DurationListened,
}
