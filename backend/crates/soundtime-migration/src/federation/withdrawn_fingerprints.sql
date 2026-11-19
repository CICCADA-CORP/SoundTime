-- A historical NAS file may have no SHA-256 at the moment of withdrawal.
-- Its publication is retired immediately. A restartable job identifies the bytes for future scans.
CREATE TABLE federation_withdrawn_fingerprints (
    track_id uuid PRIMARY KEY REFERENCES track_publications(track_id) ON DELETE CASCADE,
    expected_sha256 text,
    retry_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX federation_fingerprint_due ON federation_withdrawn_fingerprints(retry_at, track_id);
INSERT INTO federation_withdrawn_fingerprints(track_id,expected_sha256)
SELECT t.id,m.sha256 FROM tracks t JOIN track_publications p ON p.track_id=t.id
JOIN federation_catalogs c ON c.id=p.catalog_id AND c.is_local
LEFT JOIN federation_managed_originals m ON m.file_path=t.file_path
WHERE p.state='withdrawn' AND t.file_hash IS NULL AND (t.content_hash IS NOT NULL OR m.sha256 IS NOT NULL);
