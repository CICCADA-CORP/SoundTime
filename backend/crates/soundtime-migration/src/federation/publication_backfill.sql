-- Track UUID order is stable. Once the DDL commits, even a concurrent insert
-- behind the checkpoint has already executed the projection in its transaction.
CREATE OR REPLACE FUNCTION federation_backfill_batch(batch_size integer) RETURNS integer LANGUAGE plpgsql AS $$
DECLARE
    checkpoint federation_backfill%ROWTYPE;
    item tracks%ROWTYPE;
    processed integer := 0;
BEGIN
    IF batch_size IS NULL OR batch_size NOT BETWEEN 1 AND 10000 THEN RAISE EXCEPTION 'invalid batch size'; END IF;
    SELECT * INTO STRICT checkpoint FROM federation_backfill WHERE singleton FOR UPDATE;
    IF checkpoint.complete THEN RETURN 0; END IF;
    FOR item IN SELECT t.* FROM tracks t
                WHERE t.id >= COALESCE(checkpoint.last_track_id, '00000000-0000-0000-0000-000000000000'::uuid)
                  AND t.id IS DISTINCT FROM checkpoint.last_track_id
                  AND NOT EXISTS (SELECT FROM track_publications p WHERE p.track_id=t.id)
                ORDER BY t.id LIMIT batch_size FOR UPDATE OF t LOOP
        PERFORM federation_project_track(item, true);
        checkpoint.last_track_id := item.id;
        processed := processed + 1;
    END LOOP;
    UPDATE federation_backfill SET last_track_id = checkpoint.last_track_id, complete = processed < batch_size
    WHERE singleton;
    RETURN processed;
END;
$$;
