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
CREATE TABLE federation_catalog_clients (node_id text PRIMARY KEY);
CREATE INDEX federation_pex_consent ON federation_peer_policies(node_id) WHERE share_identity AND NOT is_banned;
CREATE INDEX federation_followed_catalogs ON federation_peer_policies(node_id) WHERE is_followed AND NOT is_banned;
-- Legacy administration still writes blocked_domains. Its transaction and
-- incoming catalogue commits take the same policy row lock for this identity.
CREATE FUNCTION federation_serialize_peer_block() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO federation_peer_policies(node_id) VALUES(NEW.domain)
        ON CONFLICT(node_id) DO UPDATE SET node_id=excluded.node_id;
    RETURN NULL;
END;
$$;
CREATE TRIGGER federation_block_control AFTER INSERT OR UPDATE OF domain ON blocked_domains
FOR EACH ROW EXECUTE FUNCTION federation_serialize_peer_block();

-- Keyset pagination uses the latest committed event per publication at a cut.
CREATE INDEX federation_event_snapshot ON federation_events(catalog_id,origin_publication_id,cursor DESC);
CREATE TABLE federation_event_bootstrap (
    singleton boolean PRIMARY KEY CHECK(singleton), last_track uuid, complete boolean NOT NULL DEFAULT false
);
INSERT INTO federation_event_bootstrap(singleton) VALUES(true);
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
    INSERT INTO tracks(id,title,artist_id,album_id,duration_secs,file_path,file_size,format,content_hash,
                       genre,track_number,disc_number,year,bitrate,sample_rate,musicbrainz_id)
    VALUES(p.track_id,media->>'title',artist_key,album_key,(media->>'duration_ms')::bigint / 1000.0,
           'p2p://' || asset,bytes,media->>'format',asset,media->>'genre',(media->>'track_number')::smallint,
           (media->>'disc_number')::smallint,(media->>'year')::smallint,(media->>'bitrate')::integer,
           (media->>'sample_rate')::integer,media->>'musicbrainz_id')
    ON CONFLICT(id) DO UPDATE SET title=excluded.title,artist_id=excluded.artist_id,album_id=excluded.album_id,
        duration_secs=excluded.duration_secs,file_path=excluded.file_path,file_size=excluded.file_size,
        format=excluded.format,content_hash=excluded.content_hash,genre=excluded.genre,track_number=excluded.track_number,
        disc_number=excluded.disc_number,year=excluded.year,bitrate=excluded.bitrate,sample_rate=excluded.sample_rate,
        musicbrainz_id=excluded.musicbrainz_id;
    RETURN p.track_id;
END;
$$;
