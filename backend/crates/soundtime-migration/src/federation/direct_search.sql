-- Rows expose the authoritative state up to the point at which a provider is required.
CREATE VIEW federation_track_access_scope AS
SELECT t AS metadata,t.id AS track_id,p.revision,own.node_id AS origin,own.is_local AS local_origin,
       (p.state='active' AND current_version.content_hash IS NOT NULL) AS versioned,
       CASE WHEN p.state='withdrawn' THEN 'withdrawn'
            WHEN h.track_id IS NOT NULL OR EXISTS (
                SELECT FROM federation_content_blocks b
                WHERE b.content_hash IN (current_version.content_hash,lower(t.content_hash))
            ) THEN 'unavailable_on_instance'
            WHEN p.state='detached' THEN 'temporarily_unavailable'
            WHEN m.track_id IS NULL AND t.file_path<>'' AND t.file_path !~ '^[a-zA-Z][a-zA-Z0-9+.-]*:'
                 AND (own.is_local OR p.track_id IS NULL) THEN 'playable'
       END AS terminal
FROM tracks t
LEFT JOIN track_publications p ON p.track_id=t.id
LEFT JOIN federation_catalogs own ON own.id=p.catalog_id
LEFT JOIN federation_missing_sources m ON m.track_id=t.id
LEFT JOIN federation_hidden_publications h ON h.track_id=t.id
LEFT JOIN publication_versions current_version ON current_version.track_id=p.track_id AND current_version.revision=p.revision;

-- Both versions of the transport apply these origin endpoint rules. The full provider
-- view also applies source_enabled to mirror and retained branches.
CREATE FUNCTION federation_direct_source(node text)
RETURNS TABLE(followed boolean,allowed boolean,online boolean,source_enabled boolean)
LANGUAGE sql STABLE AS $$
    SELECT coalesce(policy.is_followed,false) OR own.id IS NOT NULL,
           NOT coalesce(policy.is_banned,false) AND blocked.domain IS NULL,
           own.id IS NOT NULL OR peer.is_online IS DISTINCT FROM false,
           NOT coalesce(policy.source_disabled,false)
    FROM (VALUES(1)) anchor(v)
    LEFT JOIN federation_peer_policies policy ON policy.node_id=node
    LEFT JOIN blocked_domains blocked ON blocked.domain=node
    LEFT JOIN p2p_peers peer ON peer.node_id=node
    LEFT JOIN federation_catalogs own ON own.is_local AND own.node_id=node
$$;

CREATE VIEW federation_live_direct_offers AS
SELECT o.track_id,o.revision,o.provider_node_id AS provider,source.*
FROM publication_offers o
CROSS JOIN LATERAL federation_direct_source(o.provider_node_id) source
WHERE o.expires_at>now();

CREATE OR REPLACE VIEW federation_origin_track_providers AS
SELECT o.track_id,o.provider,o.followed,o.allowed,o.online
FROM federation_live_direct_offers o JOIN track_publications p ON p.track_id=o.track_id
JOIN publication_versions v ON v.track_id=o.track_id AND v.revision=o.revision
WHERE p.state='active' AND o.revision=p.revision AND v.content_hash IS NOT NULL
UNION ALL
SELECT t.id,substr(r.instance_domain,7),source.followed,source.allowed,source.online
FROM tracks t JOIN remote_tracks r ON r.local_track_id=t.id
LEFT JOIN track_publications p ON p.track_id=t.id
CROSS JOIN LATERAL federation_direct_source(substr(r.instance_domain,7)) source
WHERE (p.track_id IS NULL OR p.state='unresolved') AND r.is_available
  AND r.instance_domain LIKE 'p2p://%' AND t.file_path='p2p://' || t.content_hash
  AND r.remote_uri=r.instance_domain || '/' || t.content_hash
  AND NOT EXISTS(SELECT FROM federation_legacy_denied_hashes x WHERE x.content_hash=t.content_hash);

CREATE OR REPLACE FUNCTION federation_track_availability(strict_follow boolean,p2p_enabled boolean)
RETURNS TABLE(track_id uuid,availability text) LANGUAGE sql STABLE AS $$
    SELECT g.track_id,coalesce(g.terminal,CASE WHEN p2p_enabled THEN (
        SELECT CASE WHEN bool_or(s.allowed AND (NOT strict_follow OR s.followed) AND s.online) THEN 'playable'
                    WHEN bool_and(NOT s.allowed OR (strict_follow AND NOT s.followed)) THEN 'unavailable_on_instance'
                    ELSE 'temporarily_unavailable' END
        FROM federation_track_providers s WHERE s.track_id=g.track_id
    ) ELSE 'temporarily_unavailable' END)
    FROM federation_track_access_scope g
$$;
