CREATE OR REPLACE FUNCTION federation_refresh_search_batch(batch_size integer) RETURNS integer LANGUAGE plpgsql AS $$
DECLARE
    job federation_search_jobs%ROWTYPE;
    item tracks%ROWTYPE;
    page_sql text;
    processed integer := 0;
BEGIN
    IF batch_size IS NULL OR batch_size NOT BETWEEN 1 AND 10000 THEN RAISE EXCEPTION 'invalid batch size'; END IF;
    IF NOT pg_try_advisory_xact_lock(1937011565,3) THEN RETURN 0; END IF;
    -- A rename can reset this checkpoint while the batch runs. Holding its job
    -- row during the scan would invert the artist/track writer's lock order.
    SELECT * INTO job FROM federation_search_jobs ORDER BY (kind='initial') DESC, kind, subject_id LIMIT 1;
    IF NOT FOUND THEN RETURN 0; END IF;
    page_sql := 'SELECT t.* FROM tracks t ';
    IF job.kind <> 'initial' THEN
        -- The column comes from this fixed whitelist, never from a peer.
        page_sql := page_sql || 'JOIN federation_search_documents d ON d.track_id=t.id AND d.'
            || CASE job.kind WHEN 'artist' THEN 'artist_id' ELSE 'album_id' END || '=$3 ';
    END IF;
    page_sql := page_sql || 'WHERE t.id >= COALESCE($1,''00000000-0000-0000-0000-000000000000''::uuid)
                             AND t.id IS DISTINCT FROM $1 ';
    IF job.kind = 'initial' THEN
        page_sql := page_sql || 'AND NOT EXISTS (SELECT FROM federation_search_documents d WHERE d.track_id=t.id) ';
    END IF;
    page_sql := page_sql || 'ORDER BY t.id LIMIT $2 FOR SHARE OF t';
    FOR item IN EXECUTE page_sql USING job.last_track_id,batch_size,job.subject_id LOOP
        PERFORM federation_search_project(item, false);
        -- Peers need the display change even when the lexemes stayed the same.
        -- Read the last locally issued revision so overlapping artist/album jobs
        -- emit at most one revision for the same final pair of display names.
        IF job.kind <> 'initial' AND item.file_path<>'' AND item.file_path !~ '^[a-zA-Z][a-zA-Z0-9+.-]*:' THEN
            PERFORM federation_project_track(item, true);
            IF EXISTS (
                SELECT FROM track_publications p JOIN federation_catalogs c ON c.id=p.catalog_id AND c.is_local
                JOIN artists a ON a.id=item.artist_id LEFT JOIN albums al ON al.id=item.album_id
                LEFT JOIN federation_events e ON e.catalog_id=c.id AND e.origin_publication_id=p.origin_publication_id
                     AND e.revision=p.revision AND e.kind='upsert'
                WHERE p.track_id=item.id AND p.state='active' AND (e.cursor IS NULL OR
                     (e.payload->>'artist_name',e.payload->>'album_title') IS DISTINCT FROM (a.name,al.title))
            ) THEN
                PERFORM federation_project_track(item, false);
            END IF;
        END IF;
        job.last_track_id := item.id;
        processed := processed + 1;
    END LOOP;
    IF processed < batch_size THEN
        DELETE FROM federation_search_jobs WHERE kind=job.kind AND subject_id=job.subject_id AND generation=job.generation;
    ELSE
        UPDATE federation_search_jobs SET last_track_id=job.last_track_id WHERE kind=job.kind AND subject_id=job.subject_id AND generation=job.generation;
    END IF;
    RETURN processed;
END;
$$;
