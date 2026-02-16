use sea_orm_migration::prelude::*;

/// Migration 28: Normalize existing genre data.
///
/// Genre tags were stored verbatim from audio file ID3 tags, causing
/// duplicates like "disco" vs "Disco" and compound entries like
/// "Pop/R&B" or "Dance;Pop". This migration normalizes all existing
/// genre values by:
/// - Splitting on common separators (`/`, `;`, `,`, `:`) and keeping the first segment
/// - Trimming whitespace
/// - Title-casing each word (via PostgreSQL `INITCAP`)
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // Normalize genre in tracks table
        db.execute_unprepared(
            "UPDATE tracks SET genre = INITCAP(TRIM(SPLIT_PART(SPLIT_PART(SPLIT_PART(SPLIT_PART(genre, '/', 1), ';', 1), ',', 1), ':', 1))) WHERE genre IS NOT NULL AND genre != ''",
        )
        .await?;

        // Normalize genre in albums table
        db.execute_unprepared(
            "UPDATE albums SET genre = INITCAP(TRIM(SPLIT_PART(SPLIT_PART(SPLIT_PART(SPLIT_PART(genre, '/', 1), ';', 1), ',', 1), ':', 1))) WHERE genre IS NOT NULL AND genre != ''",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // No-op: original genre values cannot be restored
        Ok(())
    }
}
