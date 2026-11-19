-- Each complete, verified publication is admitted explicitly. The hash alone
-- never associates another publication with space in the cache.
CREATE TABLE federation_cached_publications (
    track_id uuid NOT NULL, revision bigint NOT NULL, content_hash text NOT NULL REFERENCES federation_assets(content_hash),
    PRIMARY KEY(track_id,revision), FOREIGN KEY(track_id,revision) REFERENCES publication_versions(track_id,revision)
);
CREATE INDEX federation_cached_hash ON federation_cached_publications(content_hash);
CREATE TABLE federation_mirror_clock(singleton boolean PRIMARY KEY CHECK(singleton),value bigint NOT NULL DEFAULT 0);
INSERT INTO federation_mirror_clock(singleton) VALUES(true);
ALTER TABLE federation_peer_policies ADD COLUMN mirror_cursor bigint NOT NULL DEFAULT 0,
    ADD COLUMN source_disabled boolean NOT NULL DEFAULT false;
CREATE TABLE federation_mirror_leases (
    track_id uuid NOT NULL, revision bigint NOT NULL, provider text NOT NULL,
    generation bigint NOT NULL CHECK(generation>0),expires_at timestamptz NOT NULL,
    active boolean NOT NULL, envelope jsonb NOT NULL, manifest jsonb NOT NULL,
    -- A node exports only its own verified copies and offers for its own origins.
    relay_cursor bigint UNIQUE,
    PRIMARY KEY(track_id,revision,provider),
    FOREIGN KEY(track_id,revision) REFERENCES publication_versions(track_id,revision)
);
CREATE INDEX federation_mirror_provider ON federation_mirror_leases(provider,expires_at);
CREATE INDEX federation_mirror_expiry ON federation_mirror_leases(expires_at);
ALTER VIEW federation_track_providers RENAME TO federation_origin_track_providers;
CREATE VIEW federation_track_providers AS
SELECT * FROM federation_origin_track_providers
UNION ALL
SELECT l.track_id,l.provider,
       (coalesce(prov.is_followed,false) OR coalesce(orig.is_followed,false) OR own.is_local) AS followed,
       (NOT coalesce(prov.is_banned,false) AND NOT coalesce(prov.source_disabled,false) AND blocked.domain IS NULL) AS allowed,
       (peer.is_online IS DISTINCT FROM false) AS online
FROM federation_mirror_leases l JOIN track_publications p ON p.track_id=l.track_id AND p.revision=l.revision AND p.state='active'
JOIN federation_catalogs own ON own.id=p.catalog_id
LEFT JOIN federation_peer_policies prov ON prov.node_id=l.provider
LEFT JOIN federation_peer_policies orig ON orig.node_id=own.node_id
LEFT JOIN blocked_domains blocked ON blocked.domain=l.provider
LEFT JOIN p2p_peers peer ON peer.node_id=l.provider
WHERE l.active AND l.expires_at>now() AND l.provider IS DISTINCT FROM (SELECT node_id FROM federation_catalogs WHERE is_local)
UNION ALL
SELECT cache.track_id,own.node_id,true,true,true FROM federation_cached_publications cache
JOIN track_publications p ON p.track_id=cache.track_id AND p.revision=cache.revision AND p.state='active'
JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision AND v.content_hash=cache.content_hash
CROSS JOIN federation_catalogs own WHERE own.is_local AND own.node_id IS NOT NULL;
