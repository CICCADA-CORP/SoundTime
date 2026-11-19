-- Rank the matching documents before resolving providers. The lateral lookup
-- checks one candidate at a time, stopping when the requested page is full.
SELECT t.*, a.name AS result_artist_name, al.title AS result_album_title, al.cover_url AS result_cover_url
FROM (
    SELECT candidate.track_id, candidate.rank
    FROM (
        SELECT s.track_id, ts_rank(s.document,to_tsquery('english',$1)) AS rank
        FROM federation_search_input s WHERE s.document @@ to_tsquery('english',$1)
        ORDER BY rank DESC,s.track_id OFFSET 0
    ) candidate
    JOIN LATERAL (
        SELECT FROM federation_track_availability($4,$5) av
        WHERE av.track_id=candidate.track_id AND av.availability='playable' OFFSET 0
    ) playable ON true
    ORDER BY candidate.rank DESC,candidate.track_id LIMIT $2 OFFSET $3
) page
JOIN tracks t ON t.id=page.track_id LEFT JOIN artists a ON a.id=t.artist_id AND NOT a.is_placeholder
LEFT JOIN albums al ON al.id=t.album_id
ORDER BY page.rank DESC,t.id
