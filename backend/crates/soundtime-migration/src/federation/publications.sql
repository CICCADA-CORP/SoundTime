-- A local catalogue has an identity in PostgreSQL even on a standalone instance.
-- The cryptographic node ID is bound only when the real key becomes available.
CREATE TABLE federation_catalogs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    is_local boolean NOT NULL DEFAULT false,
    node_id text UNIQUE,
    change_cursor bigint NOT NULL DEFAULT 0 CHECK (change_cursor >= 0),
    CHECK (is_local OR node_id IS NOT NULL)
);
CREATE UNIQUE INDEX federation_single_local_catalog ON federation_catalogs(is_local) WHERE is_local;
INSERT INTO federation_catalogs(is_local) VALUES (true);

-- Intentionally survives deletion of the original tracks row by legacy writers.
-- The UUID is the same one that playlists and other personal collections reference.
CREATE TABLE track_publications (
    track_id uuid PRIMARY KEY,
    catalog_id uuid REFERENCES federation_catalogs(id),
    origin_publication_id uuid,
    revision bigint NOT NULL CHECK (revision > 0),
    state text NOT NULL CHECK (state IN ('active', 'unresolved', 'detached', 'withdrawn')),
    CHECK ((catalog_id IS NULL) = (origin_publication_id IS NULL)),
    CHECK (state = 'detached' OR ((state = 'unresolved') = (catalog_id IS NULL))),
    UNIQUE (catalog_id, origin_publication_id)
);

-- Logical asset, independent of the number of publications and providers.
-- Knowledge of a hash is never an offer to serve its data.
CREATE TABLE federation_assets (
    content_hash text PRIMARY KEY CHECK (content_hash ~ '^[0-9a-f]{64}$'),
    file_size bigint NOT NULL CHECK (file_size >= 0)
);
CREATE TABLE publication_versions (
    track_id uuid NOT NULL REFERENCES track_publications(track_id),
    revision bigint NOT NULL CHECK (revision > 0),
    content_hash text REFERENCES federation_assets(content_hash),
    file_size bigint,
    format text NOT NULL,
    PRIMARY KEY (track_id, revision),
    CHECK (file_size IS NULL OR file_size >= 0)
);
CREATE INDEX publication_versions_by_hash ON publication_versions(content_hash) WHERE content_hash IS NOT NULL;
CREATE TABLE publication_offers (
    track_id uuid NOT NULL,
    revision bigint NOT NULL,
    provider_node_id text NOT NULL,
    expires_at timestamptz NOT NULL,
    PRIMARY KEY (track_id, revision, provider_node_id),
    FOREIGN KEY (track_id, revision) REFERENCES publication_versions(track_id, revision)
);
CREATE INDEX publication_offers_by_provider ON publication_offers(provider_node_id, expires_at);

CREATE TABLE federation_peer_policies (
    node_id text PRIMARY KEY,
    is_followed boolean NOT NULL DEFAULT false,
    is_banned boolean NOT NULL DEFAULT false,
    share_identity boolean NOT NULL DEFAULT false,
    applied_cursor bigint NOT NULL DEFAULT 0 CHECK (applied_cursor >= 0)
);
CREATE TABLE federation_content_blocks (
    content_hash text PRIMARY KEY CHECK (content_hash ~ '^[0-9a-f]{64}$'),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE federation_events (
    catalog_id uuid NOT NULL REFERENCES federation_catalogs(id),
    cursor bigint NOT NULL CHECK (cursor > 0),
    origin_publication_id uuid NOT NULL,
    revision bigint NOT NULL CHECK (revision > 0),
    kind text NOT NULL,
    payload jsonb NOT NULL,
    PRIMARY KEY (catalog_id, cursor),
    UNIQUE (catalog_id, origin_publication_id, revision, kind)
);
CREATE TABLE federation_backfill (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    last_track_id uuid,
    complete boolean NOT NULL DEFAULT false
);
INSERT INTO federation_backfill DEFAULT VALUES;

-- Hold the catalogue lock until commit. A reader cannot observe cursor N+1 while
-- the transaction producing cursor N is uncommitted. A rollback consumes no number.
CREATE FUNCTION federation_append_event(
    origin uuid, publication uuid, rev bigint, event_kind text, body jsonb
) RETURNS bigint LANGUAGE plpgsql AS $$
DECLARE
    previous federation_events%ROWTYPE;
    next_cursor bigint;
BEGIN
    SELECT change_cursor + 1 INTO STRICT next_cursor FROM federation_catalogs WHERE id = origin FOR UPDATE;
    SELECT * INTO previous FROM federation_events
     WHERE catalog_id = origin AND origin_publication_id = publication AND revision = rev AND kind = event_kind;
    IF FOUND THEN
        IF previous.payload IS DISTINCT FROM body THEN
            RAISE EXCEPTION 'conflicting event for publication % revision %', publication, rev;
        END IF;
        RETURN previous.cursor;
    END IF;
    INSERT INTO federation_events(catalog_id, cursor, origin_publication_id, revision, kind, payload)
    VALUES (origin, next_cursor, publication, rev, event_kind, body);
    UPDATE federation_catalogs SET change_cursor = next_cursor WHERE id = origin;
    RETURN next_cursor;
END;
$$;

-- Both the online trigger and the batched upgrade use the same projection. Historical
-- imports without an authenticated publication identity stay unresolved. The legacy
-- remote_tracks rows (including uncoupled MusicBrainz suggestions) remain intact.
CREATE FUNCTION federation_project_track(t tracks, historical boolean)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    local_catalog uuid;
    issuer uuid;
    source_id uuid;
    p track_publications%ROWTYPE;
    hash text;
    bytes bigint;
BEGIN
    SELECT id INTO STRICT local_catalog FROM federation_catalogs WHERE is_local;
    IF t.file_path <> '' AND t.file_path !~ '^[a-zA-Z][a-zA-Z0-9+.-]*:' THEN
        issuer := local_catalog;
        source_id := t.id;
    END IF;
    INSERT INTO track_publications(track_id, catalog_id, origin_publication_id, revision, state)
    VALUES (t.id, issuer, source_id, 1, CASE WHEN issuer IS NULL THEN 'unresolved' ELSE 'active' END)
    ON CONFLICT (track_id) DO NOTHING RETURNING * INTO p;
    IF NOT FOUND THEN
        IF historical THEN RETURN; END IF;
        -- Signed remote revisions and known withdrawals may only be changed by
        -- the publication authority. A late scan or legacy write has no authority.
        UPDATE track_publications SET revision = revision + 1
        WHERE track_id = t.id AND catalog_id = local_catalog AND state = 'active'
        RETURNING * INTO p;
        IF NOT FOUND THEN RETURN; END IF;
    END IF;
    IF t.file_size >= 0 THEN bytes := t.file_size; END IF;
    IF p.catalog_id = local_catalog AND bytes IS NOT NULL AND t.content_hash ~* '^[0-9a-f]{64}$' THEN
        hash := lower(t.content_hash);
        INSERT INTO federation_assets(content_hash, file_size) VALUES (hash, bytes) ON CONFLICT DO NOTHING;
        IF NOT EXISTS (SELECT FROM federation_assets a WHERE a.content_hash = hash AND a.file_size = bytes) THEN
            -- A contradictory legacy hint is unusable until the bytes are verified.
            hash := NULL;
        END IF;
    END IF;
    INSERT INTO publication_versions(track_id, revision, content_hash, file_size, format)
    VALUES (t.id, p.revision, hash, bytes, t.format);
    IF NOT historical AND p.catalog_id = local_catalog THEN
        PERFORM federation_append_event(local_catalog, p.origin_publication_id, p.revision, 'upsert',
            jsonb_build_object('title', t.title, 'artist_id', t.artist_id, 'album_id', t.album_id,
                'artist_name', (SELECT name FROM artists WHERE id = t.artist_id),
                'album_title', (SELECT title FROM albums WHERE id = t.album_id),
                'duration_secs', t.duration_secs, 'track_number', t.track_number, 'disc_number', t.disc_number,
                'genre', t.genre, 'year', t.year, 'musicbrainz_id', t.musicbrainz_id,
                'content_hash', hash, 'file_size', bytes, 'format', t.format,
                'bitrate', t.bitrate, 'sample_rate', t.sample_rate));
    END IF;
END;
$$;

CREATE FUNCTION federation_track_changed() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM federation_project_track(NEW, false);
    RETURN NULL;
END;
$$;
CREATE TRIGGER federation_track_insert AFTER INSERT ON tracks
FOR EACH ROW EXECUTE FUNCTION federation_track_changed();
-- Ignore play counts, waveforms, SHA-256 bookkeeping, and same-file path moves.
CREATE TRIGGER federation_track_update AFTER UPDATE OF title, artist_id, album_id, duration_secs,
    track_number, disc_number, genre, year, musicbrainz_id, content_hash, file_size, format, bitrate, sample_rate ON tracks
FOR EACH ROW WHEN (
    (NEW.title, NEW.artist_id, NEW.album_id, NEW.duration_secs, NEW.track_number, NEW.disc_number,
     NEW.genre, NEW.year, NEW.musicbrainz_id, NEW.content_hash, NEW.file_size, NEW.format, NEW.bitrate, NEW.sample_rate)
    IS DISTINCT FROM
    (OLD.title, OLD.artist_id, OLD.album_id, OLD.duration_secs, OLD.track_number, OLD.disc_number,
     OLD.genre, OLD.year, OLD.musicbrainz_id, OLD.content_hash, OLD.file_size, OLD.format, OLD.bitrate, OLD.sample_rate)
) EXECUTE FUNCTION federation_track_changed();

CREATE FUNCTION federation_track_detached() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    -- A legacy delete can also mean a temporarily missing NAS file. Preserve
    -- authority and revisions without inventing a signed owner withdrawal.
    PERFORM federation_project_track(OLD, true);
    UPDATE track_publications SET state = 'detached'
    WHERE track_id = OLD.id AND state IN ('active', 'unresolved');
    RETURN NULL;
END;
$$;
CREATE TRIGGER federation_track_delete AFTER DELETE ON tracks
FOR EACH ROW EXECUTE FUNCTION federation_track_detached();

-- Track UUID order is stable. Once the DDL commits, even a concurrent insert
-- behind the checkpoint has already executed the projection in its transaction.
CREATE FUNCTION federation_backfill_batch(batch_size integer) RETURNS integer LANGUAGE plpgsql AS $$
DECLARE
    checkpoint federation_backfill%ROWTYPE;
    item tracks%ROWTYPE;
    processed integer := 0;
BEGIN
    IF batch_size IS NULL OR batch_size NOT BETWEEN 1 AND 10000 THEN RAISE EXCEPTION 'invalid batch size'; END IF;
    SELECT * INTO STRICT checkpoint FROM federation_backfill WHERE singleton FOR UPDATE;
    IF checkpoint.complete THEN RETURN 0; END IF;
    FOR item IN SELECT * FROM tracks
                WHERE id >= COALESCE(checkpoint.last_track_id, '00000000-0000-0000-0000-000000000000'::uuid)
                  AND id IS DISTINCT FROM checkpoint.last_track_id
                ORDER BY id LIMIT batch_size FOR UPDATE LOOP
        PERFORM federation_project_track(item, true);
        checkpoint.last_track_id := item.id;
        processed := processed + 1;
    END LOOP;
    UPDATE federation_backfill SET last_track_id = checkpoint.last_track_id, complete = processed < batch_size
    WHERE singleton;
    RETURN processed;
END;
$$;
