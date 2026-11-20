-- A v2 request already identifies one signed publication and byte version.
-- The general listing view also resolves ambiguous v1 hashes across publications.
-- Many independently published tracks may reuse one hash. Resolve the exact
-- version first, then check its hash so the popular asset cannot widen this lookup.
WITH version AS MATERIALIZED (
    SELECT content_hash, file_size FROM publication_versions WHERE track_id=$1 AND revision=$4
), subject AS MATERIALIZED (
    SELECT c.is_local AS local_origin,
           (SELECT node_id FROM federation_catalogs WHERE is_local) AS local_node,
           EXISTS (SELECT FROM federation_peer_policies WHERE node_id=$2 AND is_followed) AS follows_origin
      FROM track_publications p JOIN federation_catalogs c ON c.id=p.catalog_id
      CROSS JOIN version v
     WHERE p.track_id=$1 AND p.state='active' AND c.node_id=$2
       AND p.origin_publication_id=$3 AND p.revision=$4 AND v.content_hash=$5 AND v.file_size=$6
       AND NOT EXISTS (SELECT FROM federation_hidden_publications WHERE track_id=$1)
       AND NOT EXISTS (SELECT FROM federation_content_blocks WHERE content_hash=$5)
), candidates AS (
    SELECT o.provider_node_id AS provider, false AS mirror, false AS retained
      FROM publication_offers o
     WHERE o.track_id=$1 AND o.revision=$4 AND o.expires_at>now()
       AND ($9::text IS NULL OR o.provider_node_id=$9)
    UNION ALL
    SELECT l.provider, true, false FROM federation_mirror_leases l
     WHERE l.track_id=$1 AND l.revision=$4 AND l.active AND l.expires_at>now()
       AND ($9 IS NULL OR l.provider=$9)
    UNION ALL
    SELECT t.local_node, false, true FROM subject t
     WHERE t.local_node IS NOT NULL AND ($9 IS NULL OR t.local_node=$9)
       AND (SELECT content_hash FROM federation_cached_publications
             WHERE track_id=$1 AND revision=$4)=$5
)
SELECT DISTINCT s.provider
  FROM subject t CROSS JOIN candidates s
  LEFT JOIN federation_peer_policies policy ON policy.node_id=s.provider
  LEFT JOIN p2p_peers peer ON peer.node_id=s.provider
 WHERE NOT coalesce(policy.source_disabled,false)
   AND (s.retained OR (
       NOT coalesce(policy.is_banned,false)
       AND NOT EXISTS (SELECT FROM blocked_domains WHERE domain=s.provider)
       AND (peer.is_online IS DISTINCT FROM false OR (NOT s.mirror AND s.provider=t.local_node))
       AND (NOT s.mirror OR s.provider IS DISTINCT FROM t.local_node)
       AND (NOT $7 OR coalesce(policy.is_followed,false)
            OR (s.mirror AND (t.local_origin OR t.follows_origin))
            OR (NOT s.mirror AND s.provider=t.local_node))
   ))
 ORDER BY s.provider LIMIT $8
