-- A receiver keeps one resumable snapshot per authenticated provider. Page
-- receipts detect conflicting retries. Previous offers survive an incomplete run.
CREATE TABLE federation_inventories (
    provider text PRIMARY KEY REFERENCES federation_peer_policies(node_id),
    snapshot uuid NOT NULL,
    cut bigint NOT NULL CHECK (cut >= 0),
    next_page integer NOT NULL DEFAULT 0 CHECK (next_page >= 0),
    last_publication uuid,
    started_at timestamptz NOT NULL DEFAULT now(),
    complete boolean NOT NULL DEFAULT false
);
CREATE TABLE federation_inventory_pages (
    provider text REFERENCES federation_inventories(provider) ON DELETE CASCADE,
    page integer NOT NULL CHECK (page >= 0),
    digest bytea NOT NULL CHECK (octet_length(digest)=32),
    PRIMARY KEY(provider,page)
);
CREATE TABLE federation_inventory_seen (
    provider text REFERENCES federation_inventories(provider) ON DELETE CASCADE,
    track_id uuid NOT NULL,
    revision bigint NOT NULL,
    PRIMARY KEY(provider,track_id,revision)
);
CREATE TABLE federation_signed_publications (
    track_id uuid PRIMARY KEY REFERENCES track_publications(track_id),
    envelope jsonb NOT NULL
);

-- Keyset pagination uses the latest committed event per publication at a cut.
CREATE INDEX federation_event_snapshot ON federation_events(catalog_id,origin_publication_id,cursor DESC);
CREATE TABLE federation_event_bootstrap (
    singleton boolean PRIMARY KEY CHECK(singleton), last_track uuid, complete boolean NOT NULL DEFAULT false
);
INSERT INTO federation_event_bootstrap(singleton) VALUES(true);
CREATE FUNCTION federation_seed_events(n integer) RETURNS boolean LANGUAGE plpgsql AS $$
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
            v.file_size,v.content_hash,a.name AS artist_name,l.title AS album_title
        FROM track_publications p JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision
        LEFT JOIN tracks t ON t.id=p.track_id LEFT JOIN artists a ON a.id=t.artist_id LEFT JOIN albums l ON l.id=t.album_id
        WHERE p.catalog_id=c AND (checkpoint.last_track IS NULL OR p.track_id>checkpoint.last_track)
        ORDER BY p.track_id LIMIT n LOOP
        IF item.state='active' AND item.content_hash IS NOT NULL AND item.title IS NOT NULL AND NOT EXISTS
            (SELECT FROM federation_events e WHERE e.catalog_id=c AND e.origin_publication_id=item.origin_publication_id
                  AND e.revision=item.revision)
        THEN
            PERFORM federation_append_event(c,item.origin_publication_id,item.revision,'upsert',to_jsonb(item));
        END IF;
        checkpoint.last_track := item.track_id;
        processed := processed+1;
    END LOOP;
    UPDATE federation_event_bootstrap SET last_track=checkpoint.last_track,complete=processed<n WHERE singleton;
    RETURN processed<n;
END;
$$;

-- The Rust receiver verifies the origin signature on a typed, size-limited
-- envelope before this call. This function does not create a provider offer.
CREATE FUNCTION federation_import_signed(issuer text, publication uuid, rev bigint,
                                         media jsonb, signed_envelope jsonb)
RETURNS uuid LANGUAGE plpgsql AS $$
DECLARE
    origin federation_catalogs%ROWTYPE;
    p track_publications%ROWTYPE;
    existing jsonb;
    artist_key uuid;
    album_key uuid;
    asset text;
    bytes bigint;
    is_new boolean;
BEGIN
    INSERT INTO federation_catalogs(node_id) VALUES (issuer) ON CONFLICT (node_id) DO NOTHING;
    SELECT * INTO STRICT origin FROM federation_catalogs WHERE node_id=issuer;
    -- Conflicting statements about a publication from concurrent mirror imports
    -- serialize even if neither transaction had a publication row to lock.
    PERFORM pg_advisory_xact_lock(hashtextextended(issuer || '/' || publication::text, 14901));
    SELECT * INTO p FROM track_publications
        WHERE catalog_id=origin.id AND origin_publication_id=publication FOR UPDATE;
    is_new := NOT FOUND;
    IF origin.is_local THEN
        -- A mirror can offer the local version, but never edit the origin's row.
        IF NOT is_new AND p.state='active' AND media IS NOT NULL AND p.revision=rev
           AND EXISTS(SELECT FROM publication_versions v WHERE v.track_id=p.track_id
               AND v.revision=rev AND v.content_hash=media->>'hash' AND v.file_size=(media->>'size')::bigint)
        THEN RETURN p.track_id; END IF;
        RETURN NULL;
    END IF;
    IF NOT is_new THEN
        IF rev < p.revision THEN RETURN NULL; END IF;
        IF rev=p.revision THEN
            SELECT envelope INTO existing FROM federation_signed_publications WHERE track_id=p.track_id;
            IF existing IS DISTINCT FROM signed_envelope THEN RAISE EXCEPTION 'conflicting signed publication revision'; END IF;
            RETURN p.track_id;
        END IF;
        -- A newer or replayed announcement cannot resurrect an owner withdrawal.
        IF p.state='withdrawn' THEN RETURN NULL; END IF;
    ELSE
        p.track_id := gen_random_uuid();
        INSERT INTO track_publications(track_id,catalog_id,origin_publication_id,revision,state)
        VALUES(p.track_id,origin.id,publication,rev,CASE WHEN media IS NULL THEN 'withdrawn' ELSE 'active' END);
    END IF;
    UPDATE track_publications SET revision=rev,state=CASE WHEN media IS NULL THEN 'withdrawn' ELSE 'active' END
     WHERE track_id=p.track_id;
    INSERT INTO federation_signed_publications(track_id,envelope) VALUES(p.track_id,signed_envelope)
        ON CONFLICT(track_id) DO UPDATE SET envelope=excluded.envelope;
    IF media IS NULL THEN
        DELETE FROM publication_offers WHERE track_id=p.track_id;
        RETURN p.track_id;
    END IF;
    asset := media->>'hash';
    bytes := (media->>'size')::bigint;
    INSERT INTO federation_assets(content_hash,file_size) VALUES(asset,bytes) ON CONFLICT DO NOTHING;
    IF NOT EXISTS (SELECT FROM federation_assets a WHERE a.content_hash=asset AND a.file_size=bytes)
        THEN RAISE EXCEPTION 'conflicting file size for content hash'; END IF;
    INSERT INTO publication_versions(track_id,revision,content_hash,file_size,format)
        VALUES(p.track_id,rev,asset,bytes,media->>'format');
    SELECT id INTO artist_key FROM artists WHERE name=media->>'artist' ORDER BY id LIMIT 1;
    IF artist_key IS NULL THEN
        INSERT INTO artists(id,name) VALUES(gen_random_uuid(),media->>'artist') RETURNING id INTO artist_key;
    END IF;
    IF media->>'album' IS NOT NULL THEN
        INSERT INTO albums(id,title,artist_id) VALUES(gen_random_uuid(),media->>'album',artist_key)
            ON CONFLICT(title,artist_id) DO NOTHING;
        SELECT id INTO STRICT album_key FROM albums WHERE title=media->>'album' AND artist_id=artist_key;
    END IF;
    INSERT INTO tracks(id,title,artist_id,album_id,duration_secs,file_path,file_size,format,content_hash)
    VALUES(p.track_id,media->>'title',artist_key,album_key,(media->>'duration_ms')::bigint / 1000.0,
           'p2p://' || asset,bytes,media->>'format',asset)
    ON CONFLICT(id) DO UPDATE SET title=excluded.title,artist_id=excluded.artist_id,album_id=excluded.album_id,
        duration_secs=excluded.duration_secs,file_path=excluded.file_path,file_size=excluded.file_size,
        format=excluded.format,content_hash=excluded.content_hash;
    RETURN p.track_id;
END;
$$;
