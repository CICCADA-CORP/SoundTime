-- Announce only a publication this node can actually supply.
SELECT v.content_hash AS hash, c.node_id AS authority_node, p.origin_publication_id AS publication_id,
       p.revision, t.title, CASE WHEN a.is_placeholder THEN '' ELSE a.name END AS artist_name, al.title AS album_title,
       t.duration_secs, t.format, t.genre, t.year, t.bitrate, t.musicbrainz_id,
       ts_rank(s.document, to_tsquery('english',$1)) AS rank
FROM federation_search_input s JOIN tracks t ON t.id=s.track_id
JOIN track_publications p ON p.track_id=t.id AND p.state='active'
JOIN federation_catalogs c ON c.id=p.catalog_id AND c.node_id IS NOT NULL
JOIN publication_versions v ON v.track_id=p.track_id AND v.revision=p.revision AND v.content_hash IS NOT NULL
JOIN artists a ON a.id=t.artist_id LEFT JOIN albums al ON al.id=t.album_id
LEFT JOIN federation_missing_sources m ON m.track_id=t.id
WHERE s.document @@ to_tsquery('english',$1)
  AND ((c.is_local AND m.track_id IS NULL AND t.file_path<>'' AND t.file_path !~ '^[a-zA-Z][a-zA-Z0-9+.-]*:')
       OR EXISTS(SELECT FROM publication_offers o WHERE o.track_id=p.track_id AND o.revision=p.revision
                 AND o.provider_node_id=$3 AND o.expires_at>now()))
  AND NOT EXISTS(SELECT FROM federation_content_blocks b WHERE b.content_hash IN (lower(t.content_hash),v.content_hash))
  AND NOT EXISTS(SELECT FROM federation_hidden_publications h WHERE h.track_id=p.track_id)
ORDER BY rank DESC,t.id LIMIT $2
