SELECT EXISTS (
    SELECT FROM albums WHERE to_tsvector('english', title) @@ to_tsquery('english', $1)
) AS albums, EXISTS (
    SELECT FROM artists WHERE to_tsvector('english', name) @@ to_tsquery('english', $1)
) AS artists
