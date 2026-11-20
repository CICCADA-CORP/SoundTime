CREATE OR REPLACE FUNCTION federation_append_event(
    origin uuid, publication uuid, rev bigint, event_kind text, body jsonb
) RETURNS bigint LANGUAGE plpgsql AS $$
DECLARE
    previous federation_events%ROWTYPE;
    next_cursor bigint;
BEGIN
    SELECT change_cursor + 1 INTO STRICT next_cursor FROM federation_catalogs WHERE id = origin FOR UPDATE;
    SELECT * INTO previous FROM federation_events
     WHERE catalog_id = origin AND origin_publication_id = publication AND revision = rev AND kind = event_kind;
    -- Keep replays of old, already signed revisions byte-for-byte stable.
    IF FOUND AND previous.payload = body THEN RETURN previous.cursor; END IF;
    IF event_kind='upsert' AND EXISTS (
        SELECT FROM tracks t JOIN artists a ON a.id=t.artist_id AND a.is_placeholder
        JOIN federation_catalogs c ON c.id=origin AND c.is_local WHERE t.id=publication
    ) THEN
        body := jsonb_set(body,'{artist_name}',to_jsonb(' '::text));
    END IF;
    IF previous.cursor IS NOT NULL THEN
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

CREATE OR REPLACE FUNCTION federation_import_signed(issuer text, publication uuid, rev bigint,
                                         media jsonb, signed_envelope jsonb)
RETURNS uuid LANGUAGE plpgsql AS $$
DECLARE
    origin federation_catalogs%ROWTYPE;
    p track_publications%ROWTYPE;
    existing jsonb;
    artist_key uuid;
    missing_credit boolean;
    credit_name text;
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
    -- Old signed manifests require a nonempty string. Whitespace represents an
    -- absent credit. The literal name "Unknown Artist" remains a genuine credit.
    missing_credit := (media->>'artist') ~ '^[[:space:]]*$';
    credit_name := CASE WHEN missing_credit THEN 'Unknown Artist' ELSE media->>'artist' END;
    SELECT id INTO artist_key FROM artists WHERE name=credit_name AND is_placeholder=missing_credit
        ORDER BY id LIMIT 1;
    IF artist_key IS NULL THEN
        INSERT INTO artists(id,name,is_placeholder) VALUES(gen_random_uuid(),credit_name,missing_credit) RETURNING id INTO artist_key;
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

CREATE OR REPLACE FUNCTION federation_search_project(t tracks, lock_names boolean) RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    artist_name text;
    album_title text;
BEGIN
    IF lock_names THEN
        -- Serialize an inline insert/move with renames of its new parents. Even a
        -- rename that began before this insert must see it or supply its new name.
        SELECT CASE WHEN NOT a.is_placeholder THEN a.name END INTO artist_name FROM artists a WHERE a.id=t.artist_id FOR SHARE;
        SELECT al.title INTO album_title FROM albums al WHERE al.id=t.album_id FOR SHARE;
    ELSE
        -- A concurrent rename enqueues a refresh after this worker's transaction.
        SELECT CASE WHEN NOT a.is_placeholder THEN a.name END INTO artist_name FROM artists a WHERE a.id=t.artist_id;
        SELECT al.title INTO album_title FROM albums al WHERE al.id=t.album_id;
    END IF;
    INSERT INTO federation_search_documents(track_id,artist_id,album_id,document)
    VALUES (t.id,t.artist_id,t.album_id,federation_search_vector(t.title,artist_name,album_title))
    ON CONFLICT (track_id) DO UPDATE SET artist_id=excluded.artist_id, album_id=excluded.album_id, document=excluded.document
    WHERE (federation_search_documents.artist_id, federation_search_documents.album_id, federation_search_documents.document)
        IS DISTINCT FROM (excluded.artist_id, excluded.album_id, excluded.document);
END;
$$;

CREATE OR REPLACE VIEW federation_search_input AS
    SELECT d.track_id, d.document FROM federation_search_documents d
     WHERE NOT EXISTS (SELECT FROM federation_search_jobs WHERE kind='initial')
    UNION ALL
    SELECT t.id, federation_search_vector(t.title, CASE WHEN NOT a.is_placeholder THEN a.name END, al.title)
      FROM tracks t JOIN artists a ON a.id=t.artist_id LEFT JOIN albums al ON al.id=t.album_id
     WHERE EXISTS (SELECT FROM federation_search_jobs WHERE kind='initial');

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
                     (e.payload->>'artist_name',e.payload->>'album_title') IS DISTINCT FROM (CASE WHEN a.is_placeholder THEN ' ' ELSE a.name END,al.title))
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

-- Reproject existing documents and issue a new revision only when an origin's
-- frozen display credit changes. Never rewrite a previously signed envelope.
CREATE TRIGGER federation_search_credit AFTER UPDATE OF is_placeholder ON artists FOR EACH ROW
WHEN (OLD.is_placeholder IS DISTINCT FROM NEW.is_placeholder) EXECUTE FUNCTION federation_search_renamed('artist');
INSERT INTO federation_search_jobs(kind,subject_id)
    SELECT 'artist',id FROM artists WHERE is_placeholder
    ON CONFLICT(kind,subject_id) DO UPDATE SET last_track_id=NULL,generation=federation_search_jobs.generation+1;
