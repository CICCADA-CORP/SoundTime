CREATE OR REPLACE VIEW federation_origin_track_providers AS
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

CREATE OR REPLACE FUNCTION federation_track_availability(strict_follow boolean, p2p_enabled boolean)
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
DROP VIEW federation_live_direct_offers;
DROP FUNCTION federation_direct_source(text);
DROP VIEW federation_track_access_scope;
