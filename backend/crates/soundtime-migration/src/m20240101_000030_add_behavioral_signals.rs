use sea_orm_migration::prelude::*;

/// Migration 30: Add behavioral signal columns to listen_history.
///
/// These columns capture fine-grained listening behavior for improved
/// recommendations:
/// - `source_context`: where the track was played from (album, playlist, radio, etc.)
/// - `completed`: whether the track finished naturally (ended event)
/// - `skipped`: whether the user switched to another track before completion
/// - `skip_position`: playback position (seconds) when the skip occurred
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// Add the four behavioral signal columns. Each is nullable so that
    /// existing rows are unaffected and older clients can omit them.
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ListenHistory::Table)
                    .add_column(ColumnDef::new(ListenHistory::SourceContext).string().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(ListenHistory::Table)
                    .add_column(
                        ColumnDef::new(ListenHistory::Completed)
                            .boolean()
                            .null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(ListenHistory::Table)
                    .add_column(
                        ColumnDef::new(ListenHistory::Skipped)
                            .boolean()
                            .null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(ListenHistory::Table)
                    .add_column(ColumnDef::new(ListenHistory::SkipPosition).float().null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    /// Drop the four behavioral signal columns in reverse order.
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ListenHistory::Table)
                    .drop_column(ListenHistory::SkipPosition)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(ListenHistory::Table)
                    .drop_column(ListenHistory::Skipped)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(ListenHistory::Table)
                    .drop_column(ListenHistory::Completed)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(ListenHistory::Table)
                    .drop_column(ListenHistory::SourceContext)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

/// Column identifiers for the `listen_history` table, used by Sea-ORM
/// migration helpers to generate DDL statements.
#[derive(DeriveIden)]
enum ListenHistory {
    Table,
    SourceContext,
    Completed,
    Skipped,
    SkipPosition,
}
