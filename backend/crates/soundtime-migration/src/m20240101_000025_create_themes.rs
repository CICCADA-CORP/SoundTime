use sea_orm_migration::prelude::*;

/// Migration 25: Create themes table for custom CSS theme system.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── themes table ────────────────────────────────────────────────
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS themes (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                name VARCHAR(255) NOT NULL UNIQUE,
                version VARCHAR(50) NOT NULL,
                description TEXT,
                author VARCHAR(255),
                license VARCHAR(50),
                homepage VARCHAR(500),
                git_url VARCHAR(500) NOT NULL,
                css_path VARCHAR(500) NOT NULL,
                assets_path VARCHAR(500),
                status VARCHAR(20) NOT NULL DEFAULT 'disabled',
                installed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                installed_by UUID REFERENCES users(id) ON DELETE SET NULL
            )",
        )
        .await?;

        // ── indexes ─────────────────────────────────────────────────────
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_themes_status
             ON themes(status)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS themes").await?;
        Ok(())
    }
}
