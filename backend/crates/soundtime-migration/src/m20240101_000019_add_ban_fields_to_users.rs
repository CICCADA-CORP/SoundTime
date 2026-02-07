use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .add_column(
                        ColumnDef::new(Users::IsBanned)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .add_column(ColumnDef::new(Users::BanReason).text().null())
                    .add_column(
                        ColumnDef::new(Users::BannedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .drop_column(Users::IsBanned)
                    .drop_column(Users::BanReason)
                    .drop_column(Users::BannedAt)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    IsBanned,
    BanReason,
    BannedAt,
}
