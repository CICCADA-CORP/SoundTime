-- Existing documents with a NULL stamp precede every prefix computed by this version.
-- Ranking and its MVCC snapshot must come from the same statement.
SELECT pg_current_snapshot()::text AS snapshot, to_jsonb(ARRAY(
    SELECT d.track_id FROM federation_search_documents d
    WHERE d.document @@ to_tsquery('english',$1)
    ORDER BY ts_rank(d.document,to_tsquery('english',$1)) DESC,d.track_id
    LIMIT $2
)) AS ids
WHERE NOT EXISTS (SELECT FROM federation_search_jobs WHERE kind='initial')
  AND octet_length(pg_current_snapshot()::text) <= 4096
