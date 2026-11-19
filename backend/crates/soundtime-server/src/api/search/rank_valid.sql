-- Inspect at most 33 current documents that could have changed since ranking.
-- A matching change can add or reorder a result. Rows which no longer match
-- are filtered by the live page, with a full search if its prefix is depleted.
rank_changes AS MATERIALIZED (
    (SELECT d.document FROM federation_search_documents d
     WHERE d.rank_xid >= pg_snapshot_xmax($2::text::pg_snapshot)
     ORDER BY d.rank_xid LIMIT 33)
    UNION ALL
    (SELECT changed.document FROM pg_snapshot_xip($2::text::pg_snapshot) AS active(xid)
     CROSS JOIN LATERAL (
         SELECT d.rank_xid,d.document FROM federation_search_documents d
         WHERE d.rank_xid >= active.xid ORDER BY d.rank_xid LIMIT 33
     ) changed WHERE changed.rank_xid=active.xid
     LIMIT 33)
    LIMIT 33
),
valid AS MATERIALIZED (
    SELECT NOT EXISTS (SELECT FROM federation_search_jobs WHERE kind='initial')
       AND CASE WHEN (SELECT count(*) FROM rank_changes)>32 THEN false
                ELSE NOT EXISTS (SELECT FROM rank_changes WHERE document @@ to_tsquery('english',$1))
           END AS usable
)
