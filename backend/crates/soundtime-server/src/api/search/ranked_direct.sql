,
wanted AS MATERIALIZED (
    SELECT item.id::uuid AS track_id,item.ordinal
    FROM jsonb_array_elements_text($3) WITH ORDINALITY AS item(id,ordinal)
    ORDER BY item.ordinal LIMIT ($4+$5)
),
proven AS MATERIALIZED (
    SELECT (g.metadata).*,a.name AS result_artist_name,al.title AS result_album_title,
           al.cover_url AS result_cover_url,item.ordinal
    FROM wanted item CROSS JOIN valid
    JOIN LATERAL (
        SELECT scope.metadata
        FROM federation_track_access_scope scope
        JOIN federation_search_documents d ON d.track_id=scope.track_id
        WHERE scope.track_id=item.track_id AND valid.usable AND d.document @@ to_tsquery('english',$1)
          AND coalesce(scope.terminal='playable',
              $7 AND scope.versioned AND NOT scope.local_origin AND EXISTS (
                  SELECT FROM federation_live_direct_offers o
                  WHERE o.track_id=scope.track_id AND o.revision=scope.revision AND o.provider=scope.origin
                    AND o.allowed AND o.source_enabled AND (NOT $6 OR o.followed) AND o.online
              ))
        OFFSET 0
    ) g ON true
    LEFT JOIN artists a ON a.id=(g.metadata).artist_id AND NOT a.is_placeholder
    LEFT JOIN albums al ON al.id=(g.metadata).album_id
),
proof AS MATERIALIZED (
    SELECT count(*)=least(jsonb_array_length($3),$4+$5) AS direct FROM proven
)
SELECT valid.usable,names.albums,names.artists,proof.direct,hits.*
FROM valid CROSS JOIN names CROSS JOIN proof
LEFT JOIN LATERAL (
    SELECT * FROM proven WHERE proof.direct AND ordinal>$5 ORDER BY ordinal
) hits ON true
ORDER BY hits.ordinal
