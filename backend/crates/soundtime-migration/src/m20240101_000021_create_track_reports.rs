use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(TrackReport::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TrackReport::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TrackReport::TrackId).uuid().not_null())
                    .col(ColumnDef::new(TrackReport::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(TrackReport::Reason)
                            .string_len(500)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TrackReport::Status)
                            .string_len(20)
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(TrackReport::AdminNote)
                            .string_len(500)
                            .null(),
                    )
                    .col(ColumnDef::new(TrackReport::ResolvedBy).uuid().null())
                    .col(
                        ColumnDef::new(TrackReport::ResolvedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(TrackReport::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(TrackReport::Table, TrackReport::TrackId)
                            .to(Tracks::Table, Tracks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(TrackReport::Table, TrackReport::UserId)
                            .to(Users::Table, Users::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_track_reports_status")
                    .table(TrackReport::Table)
                    .col(TrackReport::Status)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(TrackReport::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum TrackReport {
    Table,
    Id,
    TrackId,
    UserId,
    Reason,
    Status,
    AdminNote,
    ResolvedBy,
    ResolvedAt,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Tracks {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}
