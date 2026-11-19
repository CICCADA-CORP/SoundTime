-- An unversioned v1 search result is a metadata hint. An actionable result must
-- name the exact authority, publication and revision verified in our database.
WITH resolved AS (
SELECT input.ordinal,
    CASE WHEN p.state='active' AND p.revision=input.revision AND v.content_hash=input.hash
               AND av.availability='playable'
               AND EXISTS(SELECT FROM federation_track_providers source WHERE source.track_id=p.track_id
                          AND source.provider=input.source_node AND source.allowed AND source.online
                          AND (NOT $2 OR source.followed))
         THEN p.track_id END AS track_id
FROM jsonb_to_recordset($1::jsonb) AS input(ordinal int,hash text,source_node text,
                                           authority_node text,publication_id uuid,revision bigint)
LEFT JOIN federation_catalogs c ON c.node_id=input.authority_node
LEFT JOIN track_publications p ON p.catalog_id=c.id AND p.origin_publication_id=input.publication_id
LEFT JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision
LEFT JOIN federation_track_availability($2,true) av ON av.track_id=p.track_id
WHERE NOT EXISTS(SELECT FROM federation_content_blocks b WHERE b.content_hash=input.hash)
  AND NOT EXISTS(SELECT FROM blocked_domains b WHERE b.domain=input.source_node)
  AND NOT EXISTS(SELECT FROM federation_peer_policies policy WHERE policy.node_id=input.source_node AND policy.is_banned)
  AND NOT EXISTS(SELECT FROM federation_hidden_publications h WHERE h.track_id=p.track_id)
  AND p.state IS DISTINCT FROM 'withdrawn'
  AND ((p.state='active' AND p.revision=input.revision AND v.content_hash=input.hash
        AND av.availability='playable')
       OR NOT EXISTS(SELECT FROM federation_legacy_denied_hashes x WHERE x.content_hash=input.hash))
 )
SELECT resolved.ordinal, resolved.track_id, t.*, a.name AS result_artist_name,
       al.title AS result_album_title, al.cover_url AS result_cover_url
FROM resolved LEFT JOIN tracks t ON t.id=resolved.track_id
LEFT JOIN artists a ON a.id=t.artist_id LEFT JOIN albums al ON al.id=t.album_id
ORDER BY resolved.ordinal
