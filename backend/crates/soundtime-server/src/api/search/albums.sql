SELECT al.*, artist.name AS result_artist_name
FROM albums al JOIN artists artist ON artist.id=al.artist_id
WHERE to_tsvector('english',al.title) @@ to_tsquery('english',$1)
  AND EXISTS (SELECT FROM tracks t JOIN federation_track_availability($4,$5) av ON av.track_id=t.id
              WHERE t.album_id=al.id AND av.availability='playable')
ORDER BY ts_rank(to_tsvector('english',al.title),to_tsquery('english',$1)) DESC,al.id
LIMIT $2 OFFSET $3
