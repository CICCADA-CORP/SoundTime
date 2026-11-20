SELECT jsonb_build_object(
    'states', (SELECT jsonb_agg(jsonb_build_array(p.track_id, p.origin_publication_id, coalesce(c.is_local, false), p.revision, p.state) ORDER BY p.track_id)
        FROM track_publications p LEFT JOIN federation_catalogs c ON c.id = p.catalog_id),
    'versions', (SELECT jsonb_agg(jsonb_build_array(track_id, revision, content_hash, file_size, format) ORDER BY track_id, revision)
        FROM publication_versions),
    'offers', (SELECT count(*) FROM publication_offers),
    'local_without_peer_id', (SELECT count(*) FROM federation_catalogs WHERE is_local AND node_id IS NULL),
    'events', (SELECT coalesce(jsonb_agg(jsonb_build_array(cursor, origin_publication_id, revision, kind, payload) ORDER BY cursor), '[]'::jsonb)
        FROM federation_events)
) AS snapshot;
