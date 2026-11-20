ALTER TABLE p2p_peers ADD COLUMN presence_checked_at timestamptz NOT NULL DEFAULT '-infinity';
CREATE TABLE federation_legacy_copies (
    track_id uuid PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    content_hash text NOT NULL CHECK(content_hash ~ '^[0-9a-f]{64}$')
);
CREATE INDEX federation_legacy_copy_hash ON federation_legacy_copies(content_hash);
ALTER VIEW federation_track_providers RENAME TO federation_versioned_track_providers;
CREATE VIEW federation_track_providers AS
SELECT s.track_id,s.provider,s.followed,
       s.allowed AND NOT coalesce(policy.source_disabled,false) AS allowed,s.online
FROM federation_versioned_track_providers s
LEFT JOIN federation_peer_policies policy ON policy.node_id=s.provider
UNION ALL
SELECT cp.track_id, own.node_id AS provider, true AS followed, true AS allowed, true AS online
FROM federation_legacy_copies cp JOIN tracks t ON t.id=cp.track_id
JOIN track_publications p ON p.track_id=t.id AND p.state='unresolved'
CROSS JOIN federation_catalogs own
WHERE own.is_local AND own.node_id IS NOT NULL AND cp.content_hash=t.content_hash AND t.file_path='p2p://'||cp.content_hash
  AND NOT EXISTS(SELECT FROM federation_legacy_denied_hashes x WHERE x.content_hash=cp.content_hash);
