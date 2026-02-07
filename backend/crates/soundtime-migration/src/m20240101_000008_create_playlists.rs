use sea_orm_migration::prelude::*;

use super::m20240101_000001_create_users::Users;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Playlists::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Playlists::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Playlists::Name).string_len(255).not_null())
                    .col(ColumnDef::new(Playlists::Description).text().null())
                    .col(ColumnDef::new(Playlists::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(Playlists::IsPublic)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(Playlists::CoverUrl).string_len(512).null())
                    .col(ColumnDef::new(Playlists::FederationUri).string_len(512).null())
                    .col(
                        ColumnDef::new(Playlists::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Playlists::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_playlists_user_id")
                            .from(Playlists::Table, Playlists::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_playlists_user_id")
                    .table(Playlists::Table)
                    .col(Playlists::UserId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Playlists::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum Playlists {
    Table,
    Id,
    Name,
    Description,
    UserId,
    IsPublic,
    CoverUrl,
    FederationUri,
    CreatedAt,
    UpdatedAt,
}
