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
                    .table(Favorites::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Favorites::UserId).uuid().not_null())
                    .col(ColumnDef::new(Favorites::TrackId).uuid().not_null())
                    .col(
                        ColumnDef::new(Favorites::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .primary_key(
                        Index::create()
                            .col(Favorites::UserId)
                            .col(Favorites::TrackId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_favorites_user_id")
                            .from(Favorites::Table, Favorites::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_favorites_track_id")
                            .from(Favorites::Table, Favorites::TrackId)
                            .to(Tracks::Table, Tracks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Favorites::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum Favorites {
    Table,
    UserId,
    TrackId,
    CreatedAt,
}
