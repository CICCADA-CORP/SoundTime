SELECT t.*, a.name AS result_artist_name, al.title AS result_album_title,
       al.cover_url AS result_cover_url
FROM federation_search_input s
JOIN tracks t ON t.id=s.track_id
JOIN artists a ON a.id=t.artist_id
LEFT JOIN albums al ON al.id=t.album_id
JOIN federation_track_availability($4,$5) av ON av.track_id=t.id AND av.availability='playable'
WHERE s.document @@ to_tsquery('english',$1)
ORDER BY ts_rank(s.document,to_tsquery('english',$1)) DESC,t.id
LIMIT $2 OFFSET $3
