CREATE TABLE federation_missing_sources (
    track_id uuid PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE
);
CREATE TABLE federation_hidden_publications (
    track_id uuid PRIMARY KEY REFERENCES track_publications(track_id)
);
-- Only paths written by the upload handler are eligible for automatic unlinking.
-- The uploader and directory layout alone cannot distinguish an imported NAS file.
CREATE TABLE federation_managed_originals (
    file_path text PRIMARY KEY,
    sha256 text NOT NULL
);
CREATE INDEX federation_managed_hash ON federation_managed_originals(sha256);
CREATE TABLE federation_file_cleanup (
    track_id uuid PRIMARY KEY REFERENCES track_publications(track_id),
    file_path text NOT NULL,
    expected_sha256 text NOT NULL,
    retry_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX federation_cleanup_retry ON federation_file_cleanup(retry_at);
CREATE INDEX federation_cleanup_by_path ON federation_file_cleanup(file_path);
CREATE INDEX federation_tracks_by_path ON tracks(file_path);

-- Called in the same transaction as account/report changes under the catalogue
-- write lock used by uploads, volume scans and the physical cleanup worker.
CREATE FUNCTION federation_withdraw_local(target uuid) RETURNS boolean LANGUAGE plpgsql AS $$
DECLARE
    t tracks%ROWTYPE;
    p track_publications%ROWTYPE;
BEGIN
    SELECT * INTO STRICT t FROM tracks WHERE id = target FOR UPDATE;
    PERFORM federation_project_track(t, true);
    SELECT tp.* INTO STRICT p FROM track_publications tp
        JOIN federation_catalogs c ON c.id = tp.catalog_id AND c.is_local
        WHERE tp.track_id = target FOR UPDATE OF tp;
    IF p.state = 'withdrawn' THEN RETURN false; END IF;
    UPDATE track_publications SET state = 'withdrawn', revision = revision + 1
        WHERE track_id = target RETURNING * INTO p;
    DELETE FROM publication_offers WHERE track_id = target;
    PERFORM federation_append_event(p.catalog_id, p.origin_publication_id, p.revision, 'withdraw', '{}'::jsonb);
    INSERT INTO federation_file_cleanup(track_id, file_path, expected_sha256)
    SELECT target, m.file_path, m.sha256 FROM federation_managed_originals m
    WHERE m.file_path = t.file_path AND m.sha256 = t.file_hash;
    RETURN true;
END;
$$;
