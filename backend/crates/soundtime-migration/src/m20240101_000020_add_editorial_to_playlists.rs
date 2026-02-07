use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add is_editorial flag to playlists
        manager
            .alter_table(
                Table::alter()
                    .table(Playlists::Table)
                    .add_column(
                        ColumnDef::new(Playlists::IsEditorial)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;

        // Store last editorial generation timestamp in instance_settings
        // (handled via the existing key-value settings table)

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Playlists::Table)
                    .drop_column(Playlists::IsEditorial)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Playlists {
    Table,
    IsEditorial,
}
