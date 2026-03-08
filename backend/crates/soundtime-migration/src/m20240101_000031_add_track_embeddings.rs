use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Enable pgvector extension
        let db = manager.get_connection();
        db.execute_unprepared("CREATE EXTENSION IF NOT EXISTS vector")
            .await?;

        // Create track_embeddings table
        // Stores 32-dimensional feature vectors for each track,
        // used for similarity search via cosine distance.
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS track_embeddings (
                track_id UUID PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
                embedding vector(32) NOT NULL,
                metadata JSONB DEFAULT '{}',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .await?;

        // HNSW index for fast approximate nearest neighbor search
        // Using cosine distance operator (<=>)
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_track_embeddings_hnsw \
             ON track_embeddings \
             USING hnsw (embedding vector_cosine_ops) \
             WITH (m = 16, ef_construction = 64)",
        )
        .await?;

        // Create user_taste_vectors table
        // Stores per-user taste profiles as weighted averages of
        // their listened track embeddings.
        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS user_taste_vectors (
                user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
                taste_vector vector(32) NOT NULL,
                listen_count INTEGER NOT NULL DEFAULT 0,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS user_taste_vectors")
            .await?;
        db.execute_unprepared("DROP INDEX IF EXISTS idx_track_embeddings_hnsw")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS track_embeddings")
            .await?;
        // Don't drop the extension — other things might use it
        Ok(())
    }
}
