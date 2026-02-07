use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add uploaded_by column to tracks table (nullable for existing rows)
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("tracks"))
                    .add_column(
                        ColumnDef::new(Alias::new("uploaded_by"))
                            .uuid()
                            .null(),
                    )
                    .add_foreign_key(
                        &TableForeignKey::new()
                            .name("fk_tracks_uploaded_by")
                            .from_tbl(Alias::new("tracks"))
                            .from_col(Alias::new("uploaded_by"))
                            .to_tbl(Alias::new("users"))
                            .to_col(Alias::new("id"))
                            .on_delete(ForeignKeyAction::SetNull)
                            .to_owned(),
                    )
                    .to_owned(),
            )
            .await?;

        // Add play_count column to tracks for fast sorting
        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("tracks"))
                    .add_column(
                        ColumnDef::new(Alias::new("play_count"))
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        // Index for sorting by play_count
        manager
            .create_index(
                Index::create()
                    .name("idx_tracks_play_count")
                    .table(Alias::new("tracks"))
                    .col(Alias::new("play_count"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_tracks_play_count")
                    .table(Alias::new("tracks"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("tracks"))
                    .drop_column(Alias::new("play_count"))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Alias::new("tracks"))
                    .drop_foreign_key(Alias::new("fk_tracks_uploaded_by"))
                    .drop_column(Alias::new("uploaded_by"))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
