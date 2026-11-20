//! Check the durable indexing queue with a pre-hashed v34 original and a new import.
use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use std::error::Error;

pub async fn check(db: &DatabaseConnection) -> Result<(), Box<dyn Error>> {
    let tx = db.begin().await?;
    tx.execute_unprepared(
        r#"DO $$
        DECLARE
            historical uuid := '30000000-0000-4000-8000-000000000001';
            remote uuid := '30000000-0000-4000-8000-000000000002';
            imported uuid := '30000000-0000-4000-8000-000000000000';
        BEGIN
            IF NOT EXISTS (SELECT FROM pg_attribute WHERE attrelid='federation_original_proofs'::regclass
                           AND attname='outboard' AND NOT attisdropped AND attstorage='e') THEN
                RAISE EXCEPTION 'outboard byte ranges require EXTERNAL storage';
            END IF;
            IF NOT EXISTS (SELECT FROM federation_original_index q JOIN tracks t ON t.id=q.track_id
                           WHERE t.id=historical AND t.content_hash IS NOT NULL AND q.retry_at<=now())
               OR NOT EXISTS (SELECT FROM federation_original_index WHERE track_id=imported)
               OR EXISTS (SELECT FROM federation_original_index WHERE track_id=remote) THEN
                RAISE EXCEPTION 'legacy hash, new local import or remote URI incorrectly queued';
            END IF;
            DELETE FROM federation_original_index WHERE track_id=historical;
            UPDATE tracks SET file_path='scanned/moved.wav' WHERE id=historical;
            IF NOT EXISTS (SELECT FROM federation_original_index WHERE track_id=historical) THEN
                RAISE EXCEPTION 'moved local original lost its indexing job';
            END IF;
            INSERT INTO federation_missing_sources(track_id) VALUES(historical);
            IF EXISTS (SELECT FROM federation_original_index WHERE track_id=historical) THEN
                RAISE EXCEPTION 'missing original remained queued';
            END IF;
            DELETE FROM federation_missing_sources WHERE track_id=historical;
            IF NOT EXISTS (SELECT FROM federation_original_index WHERE track_id=historical AND retry_at<=now()) THEN
                RAISE EXCEPTION 'restored source with historical hash but no proof was not requeued';
            END IF;
            INSERT INTO federation_original_proofs(track_id,content_hash,file_size,outboard)
                SELECT id,content_hash,file_size,''::bytea FROM tracks WHERE id=historical;
            IF NOT federation_withdraw_local(historical) THEN RAISE EXCEPTION 'withdrawal failed'; END IF;
            UPDATE tracks SET file_path='scanned/returned.wav' WHERE id=historical;
            IF EXISTS (SELECT FROM federation_original_index WHERE track_id=historical)
               OR EXISTS (SELECT FROM federation_original_proofs WHERE track_id=historical) THEN
                RAISE EXCEPTION 'withdrawn publication retained its proof or indexing job';
            END IF;
        END $$"#,
    )
    .await?;
    tx.rollback().await?;
    Ok(())
}
