CREATE FUNCTION federation_search_vector(track_title text, artist_name text, album_title text)
RETURNS tsvector LANGUAGE sql IMMUTABLE PARALLEL SAFE AS $$
    SELECT setweight(to_tsvector('english', coalesce(track_title,'')), 'A')
        || setweight(to_tsvector('english', coalesce(artist_name,'')), 'B')
        || setweight(to_tsvector('english', coalesce(album_title,'')), 'C')
$$;
CREATE TABLE federation_search_documents (
    track_id uuid PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    artist_id uuid NOT NULL,
    album_id uuid,
    document tsvector NOT NULL
);
CREATE INDEX federation_search_document_gin ON federation_search_documents USING gin(document);
CREATE INDEX federation_search_artist_members ON federation_search_documents(artist_id, track_id);
CREATE INDEX federation_search_album_members ON federation_search_documents(album_id, track_id) WHERE album_id IS NOT NULL;

-- Rename events are independent of catalogue sizes. Workers refresh a bounded
-- page per transaction. A second rename resets the checkpoint under the row lock.
CREATE TABLE federation_search_jobs (
    kind text NOT NULL CHECK(kind IN ('initial', 'artist', 'album')),
    subject_id uuid NOT NULL,
    last_track_id uuid,
    generation bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (kind, subject_id)
);
INSERT INTO federation_search_jobs(kind,subject_id) VALUES ('initial','00000000-0000-0000-0000-000000000000');
CREATE FUNCTION federation_search_renamed() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO federation_search_jobs(kind, subject_id) VALUES (TG_ARGV[0], NEW.id)
    ON CONFLICT (kind, subject_id) DO UPDATE SET last_track_id=NULL, generation=federation_search_jobs.generation+1;
    RETURN NULL;
END;
$$;
CREATE TRIGGER federation_search_artist AFTER UPDATE OF name ON artists FOR EACH ROW
WHEN (OLD.name IS DISTINCT FROM NEW.name) EXECUTE FUNCTION federation_search_renamed('artist');
CREATE TRIGGER federation_search_album AFTER UPDATE OF title ON albums FOR EACH ROW
WHEN (OLD.title IS DISTINCT FROM NEW.title) EXECUTE FUNCTION federation_search_renamed('album');

CREATE FUNCTION federation_search_project(t tracks, lock_names boolean) RETURNS void LANGUAGE plpgsql AS $$
DECLARE
    artist_name text;
    album_title text;
BEGIN
    IF lock_names THEN
        -- Serialize an inline insert/move with renames of its new parents. Even a
        -- rename that began before this insert must see it or supply its new name.
        SELECT a.name INTO artist_name FROM artists a WHERE a.id=t.artist_id FOR SHARE;
        SELECT al.title INTO album_title FROM albums al WHERE al.id=t.album_id FOR SHARE;
    ELSE
        -- A concurrent rename enqueues a refresh after this worker's transaction.
        SELECT a.name INTO artist_name FROM artists a WHERE a.id=t.artist_id;
        SELECT al.title INTO album_title FROM albums al WHERE al.id=t.album_id;
    END IF;
    INSERT INTO federation_search_documents(track_id,artist_id,album_id,document)
    VALUES (t.id,t.artist_id,t.album_id,federation_search_vector(t.title,artist_name,album_title))
    ON CONFLICT (track_id) DO UPDATE SET artist_id=excluded.artist_id, album_id=excluded.album_id, document=excluded.document
    WHERE (federation_search_documents.artist_id, federation_search_documents.album_id, federation_search_documents.document)
        IS DISTINCT FROM (excluded.artist_id, excluded.album_id, excluded.document);
END;
$$;
CREATE FUNCTION federation_search_track_changed() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    PERFORM federation_search_project(NEW, true);
    RETURN NULL;
END;
$$;
CREATE TRIGGER federation_search_track_insert AFTER INSERT ON tracks FOR EACH ROW EXECUTE FUNCTION federation_search_track_changed();
CREATE TRIGGER federation_search_track_update AFTER UPDATE OF title, artist_id, album_id ON tracks FOR EACH ROW
WHEN ((OLD.title,OLD.artist_id,OLD.album_id) IS DISTINCT FROM (NEW.title,NEW.artist_id,NEW.album_id))
EXECUTE FUNCTION federation_search_track_changed();

-- Serve the existing library throughout the first backfill. After completion the
-- initial branch has a false one-time predicate and the GIN path answers queries.
CREATE VIEW federation_search_input AS
    SELECT d.track_id, d.document FROM federation_search_documents d
     WHERE NOT EXISTS (SELECT FROM federation_search_jobs WHERE kind='initial')
    UNION ALL
    SELECT t.id, federation_search_vector(t.title, a.name, al.title)
      FROM tracks t JOIN artists a ON a.id=t.artist_id LEFT JOIN albums al ON al.id=t.album_id
     WHERE EXISTS (SELECT FROM federation_search_jobs WHERE kind='initial');

-- A v1 raw-hash read cannot disambiguate two publications. Keep the restriction
-- narrowly on the legacy transport; an independent authorized v2 offer can remain.
CREATE VIEW federation_legacy_denied_hashes AS
    SELECT v.content_hash FROM publication_versions v JOIN track_publications p USING(track_id)
    LEFT JOIN federation_hidden_publications h USING(track_id)
    WHERE v.content_hash IS NOT NULL AND (p.state='withdrawn' OR h.track_id IS NOT NULL)
    UNION ALL
    SELECT lower(t.content_hash) FROM tracks t LEFT JOIN track_publications p ON p.track_id=t.id
    LEFT JOIN federation_hidden_publications h ON h.track_id=t.id
    WHERE t.content_hash IS NOT NULL AND (p.state='withdrawn' OR h.track_id IS NOT NULL);

CREATE VIEW federation_track_providers AS
SELECT candidate.track_id, candidate.provider,
       (coalesce(policy.is_followed,false) OR own.id IS NOT NULL) AS followed,
       (NOT coalesce(policy.is_banned,false) AND b.id IS NULL) AS allowed,
       (own.id IS NOT NULL OR peer.is_online IS DISTINCT FROM false) AS online
FROM (
    SELECT o.track_id, o.provider_node_id AS provider
    FROM publication_offers o JOIN track_publications p ON p.track_id=o.track_id
    JOIN publication_versions v ON v.track_id=o.track_id AND v.revision=o.revision
    WHERE p.state='active' AND o.revision=p.revision AND v.content_hash IS NOT NULL AND o.expires_at>now()
    UNION ALL
    SELECT t.id, substr(r.instance_domain,7)
    FROM tracks t JOIN remote_tracks r ON r.local_track_id=t.id
    LEFT JOIN track_publications p ON p.track_id=t.id
    WHERE (p.track_id IS NULL OR p.state='unresolved') AND r.is_available
      AND r.instance_domain LIKE 'p2p://%' AND t.file_path='p2p://' || t.content_hash
      AND r.remote_uri=r.instance_domain || '/' || t.content_hash
      AND NOT EXISTS(SELECT FROM federation_legacy_denied_hashes x WHERE x.content_hash=t.content_hash)
) candidate
LEFT JOIN federation_peer_policies policy ON policy.node_id=candidate.provider
LEFT JOIN blocked_domains b ON b.domain=candidate.provider
LEFT JOIN p2p_peers peer ON peer.node_id=candidate.provider
LEFT JOIN federation_catalogs own ON own.is_local AND own.node_id=candidate.provider;

-- SQL table function inlines into list/search queries. Always resolve actual
-- authorization again at the streaming boundary. A catalogue hint is not proof
-- that a cache file is present.
CREATE FUNCTION federation_track_availability(strict_follow boolean, p2p_enabled boolean)
RETURNS TABLE(track_id uuid, availability text) LANGUAGE sql STABLE AS $$
    SELECT t.id, CASE
        WHEN p.state = 'withdrawn' THEN 'withdrawn'
        WHEN h.track_id IS NOT NULL OR EXISTS (SELECT FROM federation_content_blocks b WHERE b.content_hash IN (current_version.content_hash, lower(t.content_hash))) THEN 'unavailable_on_instance'
        WHEN p.state='detached' THEN 'temporarily_unavailable'
        WHEN m.track_id IS NULL AND t.file_path<>'' AND t.file_path !~ '^[a-zA-Z][a-zA-Z0-9+.-]*:'
             AND (own.is_local OR p.track_id IS NULL) THEN 'playable'
        WHEN p2p_enabled THEN (
            SELECT CASE WHEN bool_or(s.allowed AND (NOT strict_follow OR s.followed) AND s.online) THEN 'playable'
                   WHEN bool_and(NOT s.allowed OR (strict_follow AND NOT s.followed)) THEN 'unavailable_on_instance'
                   ELSE 'temporarily_unavailable' END FROM federation_track_providers s WHERE s.track_id=t.id
        )
        ELSE 'temporarily_unavailable' END
    FROM tracks t
    LEFT JOIN track_publications p ON p.track_id=t.id
    LEFT JOIN federation_catalogs own ON own.id=p.catalog_id
    LEFT JOIN federation_missing_sources m ON m.track_id=t.id
    LEFT JOIN federation_hidden_publications h ON h.track_id=t.id
    LEFT JOIN publication_versions current_version ON current_version.track_id=p.track_id AND current_version.revision=p.revision
$$;
