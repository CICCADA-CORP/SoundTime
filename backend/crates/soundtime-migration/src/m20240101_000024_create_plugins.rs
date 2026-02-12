use sea_orm_migration::prelude::*;

/// Migration 24: Create plugin system tables.
///
/// Three tables:
/// - `plugins`: installed plugin metadata and status
/// - `plugin_configs`: per-plugin key-value configuration store
/// - `plugin_events_log`: audit log of plugin event executions
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        // ── plugins table ─────────────────────────────────────────────
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS plugins (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                name VARCHAR(255) NOT NULL UNIQUE,
                version VARCHAR(50) NOT NULL,
                description TEXT,
                author VARCHAR(255),
                license VARCHAR(50),
                homepage VARCHAR(500),
                git_url VARCHAR(500) NOT NULL,
                wasm_path VARCHAR(500) NOT NULL,
                permissions JSONB NOT NULL DEFAULT '{}',
                status VARCHAR(20) NOT NULL DEFAULT 'disabled',
                error_message TEXT,
                installed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                installed_by UUID REFERENCES users(id) ON DELETE SET NULL
            )",
        )
        .await?;

        // ── plugin_configs table ──────────────────────────────────────
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS plugin_configs (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                plugin_id UUID NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
                key VARCHAR(255) NOT NULL,
                value TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(plugin_id, key)
            )",
        )
        .await?;

        // ── plugin_events_log table ───────────────────────────────────
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS plugin_events_log (
                id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                plugin_id UUID NOT NULL REFERENCES plugins(id) ON DELETE CASCADE,
                event_name VARCHAR(100) NOT NULL,
                payload JSONB,
                result VARCHAR(20) NOT NULL,
                execution_time_ms INTEGER NOT NULL,
                error_message TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .await?;

        // ── indexes ───────────────────────────────────────────────────
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_plugin_events_log_plugin_id
             ON plugin_events_log(plugin_id)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_plugin_events_log_created_at
             ON plugin_events_log(created_at)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS plugin_events_log")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS plugin_configs")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS plugins")
            .await?;
        Ok(())
    }
}
