-- The caller provides a one-row names CTE. Both negative and empty pages
-- return that row, so name discovery does not need another pool acquisition.
valid AS MATERIALIZED (
    SELECT NOT EXISTS (SELECT FROM federation_search_jobs WHERE kind='initial')
       AND coalesce((SELECT rank_xid < pg_snapshot_xmax($2::text::pg_snapshot)
                     FROM federation_search_documents WHERE rank_xid IS NOT NULL
                     ORDER BY rank_xid DESC LIMIT 1), true)
       AND NOT EXISTS (SELECT FROM pg_snapshot_xip($2::text::pg_snapshot) AS active(xid)
                       CROSS JOIN LATERAL (
                           SELECT d.rank_xid FROM federation_search_documents d
                           WHERE d.rank_xid >= active.xid ORDER BY d.rank_xid LIMIT 1
                       ) next_change WHERE next_change.rank_xid=active.xid) AS usable
)
