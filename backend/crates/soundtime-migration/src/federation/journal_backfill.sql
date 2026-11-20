CREATE OR REPLACE FUNCTION federation_seed_events(n integer) RETURNS boolean LANGUAGE plpgsql AS $$
DECLARE
    c uuid;
    checkpoint federation_event_bootstrap%ROWTYPE;
    item record;
    processed integer := 0;
BEGIN
    IF n IS NULL OR n NOT BETWEEN 1 AND 256 THEN RAISE EXCEPTION 'invalid seed batch'; END IF;
    IF NOT (SELECT complete FROM federation_backfill WHERE singleton) THEN RETURN false; END IF;
    SELECT * INTO STRICT checkpoint FROM federation_event_bootstrap WHERE singleton FOR UPDATE;
    IF checkpoint.complete THEN RETURN true; END IF;
    SELECT id INTO STRICT c FROM federation_catalogs WHERE is_local FOR UPDATE;
    FOR item IN SELECT p.track_id,p.origin_publication_id,p.revision,p.state,t.title,t.duration_secs,t.format,
            t.genre,t.track_number,t.disc_number,t.year,t.bitrate,t.sample_rate,t.musicbrainz_id,
            v.file_size,v.content_hash,a.name AS artist_name,l.title AS album_title
        FROM track_publications p JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision
        JOIN tracks t ON t.id=p.track_id LEFT JOIN artists a ON a.id=t.artist_id LEFT JOIN albums l ON l.id=t.album_id
        WHERE p.catalog_id=c AND p.state='active' AND v.content_hash IS NOT NULL AND t.title IS NOT NULL
          AND p.track_id >= COALESCE(checkpoint.last_track, '00000000-0000-0000-0000-000000000000'::uuid)
          AND p.track_id IS DISTINCT FROM checkpoint.last_track
          AND NOT EXISTS (SELECT FROM federation_events e WHERE e.catalog_id=c
                          AND e.origin_publication_id=p.origin_publication_id AND e.revision=p.revision)
        ORDER BY p.track_id LIMIT n LOOP
        PERFORM federation_append_event(c,item.origin_publication_id,item.revision,'upsert',to_jsonb(item));
        checkpoint.last_track := item.track_id;
        processed := processed+1;
    END LOOP;
    UPDATE federation_event_bootstrap SET last_track=checkpoint.last_track,complete=processed<n WHERE singleton;
    RETURN processed<n;
END;
$$;
