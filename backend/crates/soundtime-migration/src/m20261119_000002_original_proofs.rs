use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared("
            CREATE TABLE federation_original_proofs (
                track_id uuid PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
                content_hash text NOT NULL,
                file_size bigint NOT NULL CHECK (file_size >= 0),
                outboard bytea NOT NULL
            );
            CREATE INDEX federation_original_proofs_hash ON federation_original_proofs(content_hash);
            ALTER TABLE federation_original_proofs ALTER COLUMN outboard SET STORAGE EXTERNAL;
            CREATE TABLE federation_original_index (
                track_id uuid PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
                retry_at timestamptz NOT NULL DEFAULT now()
            );
            CREATE INDEX federation_original_index_ready ON federation_original_index(retry_at,track_id);
            INSERT INTO federation_original_index(track_id)
                SELECT t.id FROM tracks t WHERE t.file_path<>'' AND t.file_path !~ '^[a-zA-Z][a-zA-Z0-9+.-]*:'
                AND NOT EXISTS (SELECT FROM federation_missing_sources m WHERE m.track_id=t.id)
                AND NOT EXISTS (SELECT FROM track_publications p WHERE p.track_id=t.id AND p.state='withdrawn');
            CREATE FUNCTION federation_queue_original() RETURNS trigger LANGUAGE plpgsql AS $$
            DECLARE target uuid;
            BEGIN
                IF TG_TABLE_NAME='tracks' THEN target:=NEW.id; ELSE target:=OLD.track_id; END IF;
                -- New uploads and scans normally bind their proof immediately after commit.
                -- Delay their recovery job so the background worker cannot start the same read.
                INSERT INTO federation_original_index(track_id,retry_at)
                    SELECT t.id, CASE WHEN TG_OP='INSERT' THEN now()+interval '2 minutes' ELSE now() END
                    FROM tracks t WHERE t.id=target AND t.file_path<>'' AND t.file_path !~ '^[a-zA-Z][a-zA-Z0-9+.-]*:'
                    AND NOT EXISTS (SELECT FROM federation_missing_sources m WHERE m.track_id=t.id)
                    AND NOT EXISTS (SELECT FROM track_publications p WHERE p.track_id=t.id AND p.state='withdrawn')
                    AND NOT EXISTS (SELECT FROM federation_original_proofs f WHERE f.track_id=t.id
                                    AND f.content_hash=t.content_hash AND f.file_size=t.file_size)
                    ON CONFLICT(track_id) DO UPDATE SET retry_at=now();
                RETURN NULL;
            END $$;
            CREATE TRIGGER federation_original_path AFTER INSERT OR UPDATE OF file_path ON tracks
                FOR EACH ROW WHEN (NEW.file_path<>'' AND NEW.file_path !~ '^[a-zA-Z][a-zA-Z0-9+.-]*:')
                EXECUTE FUNCTION federation_queue_original();
            CREATE TRIGGER federation_original_return AFTER DELETE ON federation_missing_sources
                FOR EACH ROW EXECUTE FUNCTION federation_queue_original();
            CREATE FUNCTION federation_dequeue_missing_original() RETURNS trigger LANGUAGE plpgsql AS $$
            BEGIN
                DELETE FROM federation_original_index WHERE track_id=NEW.track_id;
                RETURN NULL;
            END $$;
            CREATE TRIGGER federation_original_lost AFTER INSERT ON federation_missing_sources
                FOR EACH ROW EXECUTE FUNCTION federation_dequeue_missing_original();
            CREATE FUNCTION federation_forget_original_proof() RETURNS trigger LANGUAGE plpgsql AS $$
            BEGIN
                DELETE FROM federation_original_proofs WHERE track_id=NEW.track_id;
                DELETE FROM federation_original_index WHERE track_id=NEW.track_id;
                RETURN NEW;
            END $$;
            CREATE TRIGGER federation_forget_original_proof AFTER UPDATE OF state ON track_publications
                FOR EACH ROW WHEN (NEW.state='withdrawn' AND NEW.state IS DISTINCT FROM OLD.state)
                EXECUTE FUNCTION federation_forget_original_proof();
            CREATE TRIGGER fence_federation_writer BEFORE INSERT OR UPDATE OR DELETE ON federation_original_index
                FOR EACH STATEMENT EXECUTE FUNCTION federation_check_writer();
            CREATE TRIGGER fence_federation_writer BEFORE INSERT OR UPDATE OR DELETE ON federation_original_proofs
                FOR EACH STATEMENT EXECUTE FUNCTION federation_check_writer();
        ").await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                "
            DO $$ BEGIN
                IF EXISTS (SELECT FROM federation_original_proofs) THEN
                    RAISE EXCEPTION 'stored proofs are required to serve published originals';
                END IF;
            END $$;
            DROP TRIGGER federation_forget_original_proof ON track_publications;
            DROP FUNCTION federation_forget_original_proof();
            DROP TRIGGER federation_original_lost ON federation_missing_sources;
            DROP FUNCTION federation_dequeue_missing_original();
            DROP TRIGGER federation_original_path ON tracks;
            DROP TRIGGER federation_original_return ON federation_missing_sources;
            DROP FUNCTION federation_queue_original();
            DROP TABLE federation_original_index;
            DROP TABLE federation_original_proofs;
        ",
            )
            .await?;
        Ok(())
    }
}
