SELECT valid.usable, names.albums, names.artists, hits.*
FROM valid CROSS JOIN names
LEFT JOIN LATERAL (
    SELECT t.*, a.name AS result_artist_name, al.title AS result_album_title,
           al.cover_url AS result_cover_url, page.ordinal
    FROM (
        SELECT candidate.track_id, candidate.ordinal
        FROM (
            SELECT item.id::uuid AS track_id, item.ordinal
            FROM jsonb_array_elements_text($3) WITH ORDINALITY AS item(id,ordinal)
            ORDER BY item.ordinal OFFSET 0
        ) candidate
        JOIN LATERAL (
            SELECT FROM federation_search_documents d
            JOIN federation_track_availability($6,$7) av ON av.track_id=d.track_id
            WHERE d.track_id=candidate.track_id AND d.document @@ to_tsquery('english',$1)
              AND av.availability='playable' OFFSET 0
        ) playable ON true
        WHERE valid.usable
        ORDER BY candidate.ordinal LIMIT $4 OFFSET $5
    ) page
    JOIN tracks t ON t.id=page.track_id LEFT JOIN artists a ON a.id=t.artist_id AND NOT a.is_placeholder
    LEFT JOIN albums al ON al.id=t.album_id
) hits ON true
ORDER BY hits.ordinal
