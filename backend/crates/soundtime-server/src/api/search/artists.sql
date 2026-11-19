SELECT a.* FROM artists a
WHERE to_tsvector('english',a.name) @@ to_tsquery('english',$1)
  AND (EXISTS (SELECT FROM tracks t JOIN federation_track_availability($4,$5) av ON av.track_id=t.id
               WHERE t.artist_id=a.id AND av.availability='playable')
       OR EXISTS(SELECT FROM albums al JOIN tracks t ON t.album_id=al.id
                 JOIN federation_track_availability($4,$5) av ON av.track_id=t.id
                 WHERE al.artist_id=a.id AND av.availability='playable'))
ORDER BY ts_rank(to_tsvector('english',a.name),to_tsquery('english',$1)) DESC,a.id
LIMIT $2 OFFSET $3
