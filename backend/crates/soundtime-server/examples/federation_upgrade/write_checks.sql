DO $$
DECLARE
    p uuid := '30000000-0000-4000-8000-000000000000';
    q uuid := '30000000-0000-4000-8000-000000000001';
    h text := repeat('a', 64);
    rev bigint;
BEGIN
    UPDATE tracks SET content_hash = h, file_size = 48 WHERE id IN (p, q);
    IF (SELECT count(*) FROM federation_assets WHERE content_hash = h AND file_size = 48) <> 1
       OR (SELECT count(*) FROM publication_versions WHERE track_id IN (p,q) AND content_hash = h) <> 2 THEN
        RAISE EXCEPTION 'equal bytes did not retain two publication versions and one asset';
    END IF;
    SELECT revision INTO STRICT rev FROM track_publications WHERE track_id = p;
    UPDATE tracks SET play_count = play_count + 1 WHERE id = p;
    IF (SELECT revision FROM track_publications WHERE track_id = p) <> rev THEN
        RAISE EXCEPTION 'listening changed the publication revision';
    END IF;
    UPDATE track_publications SET state = 'withdrawn' WHERE track_id = p;
    UPDATE tracks SET title = 'Late scanner write' WHERE id = p;
    DELETE FROM tracks WHERE id = p;
    IF NOT EXISTS (SELECT FROM track_publications WHERE track_id = p AND state = 'withdrawn' AND revision = rev) THEN
        RAISE EXCEPTION 'legacy writer changed a known withdrawal';
    END IF;
    DELETE FROM tracks WHERE id = q;
    IF NOT EXISTS (SELECT FROM track_publications WHERE track_id = q AND origin_publication_id = q AND state = 'detached') THEN
        RAISE EXCEPTION 'legacy deletion lost provenance or claimed to be an authority withdrawal';
    END IF;
END;
$$;
