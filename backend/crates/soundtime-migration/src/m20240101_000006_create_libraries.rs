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
                    .table(Libraries::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Libraries::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Libraries::Name).string_len(255).not_null())
                    .col(ColumnDef::new(Libraries::Description).text().null())
                    .col(ColumnDef::new(Libraries::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(Libraries::IsPublic)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Libraries::FederationUri)
                            .string_len(512)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Libraries::TotalTracks)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Libraries::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_libraries_user_id")
                            .from(Libraries::Table, Libraries::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_libraries_user_id")
                    .table(Libraries::Table)
                    .col(Libraries::UserId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Libraries::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
pub enum Libraries {
    Table,
    Id,
    Name,
    Description,
    UserId,
    IsPublic,
    FederationUri,
    TotalTracks,
    CreatedAt,
}
