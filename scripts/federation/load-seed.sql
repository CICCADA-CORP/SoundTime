-- Disposable db-b only. Virtual authorities provide searchable metadata without QUIC traffic.
\set ON_ERROR_STOP on
\if :fenced
SELECT set_config('application_name', :'worker', false) AS fixture_owner \gset
\endif
BEGIN;
CREATE TEMP TABLE pilot_origins AS
SELECT s, md5('load-catalog-'||s)::uuid AS id, 'synthetic-pilot-'||s AS node
FROM generate_series(1,:origins) s;
INSERT INTO artists(id,name) VALUES(md5('load-artist')::uuid,'Pilot Ensemble');
CREATE TEMP TABLE pilot_rows AS
SELECT o.id AS catalog,o.node,md5(o.node||':'||n)::uuid AS id,
       md5('data-'||o.node||':'||n)||md5(o.node||':'||n||'-bao') AS hash,
       'Quartz'||lpad(o.s::text,4,'0')||' Melody'||lpad((n%20)::text,2,'0')||' '||n AS title
FROM pilot_origins o CROSS JOIN generate_series(1,:titles) n;
CREATE UNIQUE INDEX ON pilot_rows(id);
CREATE INDEX ON pilot_rows(catalog);
ANALYZE pilot_rows;
COMMIT;

-- One transaction per authority. The database's normal writer triggers apply
-- throughout. A stale fixture writer cannot continue after losing its lease.
CREATE PROCEDURE pg_temp.seed_origin(authority uuid)
LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO federation_catalogs(id,is_local,node_id)
        SELECT id,false,node FROM pilot_origins WHERE id=authority;
    INSERT INTO federation_peer_policies(node_id,is_followed)
        SELECT node,true FROM pilot_origins WHERE id=authority;
    INSERT INTO tracks(id,title,artist_id,file_path,file_size,duration_secs,format,content_hash)
        SELECT id,title,md5('load-artist')::uuid,'p2p://'||hash,8192,1,'wav',hash
        FROM pilot_rows WHERE catalog=authority;
    INSERT INTO federation_assets(content_hash,file_size)
        SELECT hash,8192 FROM pilot_rows WHERE catalog=authority;
    UPDATE track_publications p SET catalog_id=r.catalog,origin_publication_id=r.id,state='active'
        FROM pilot_rows r WHERE r.catalog=authority AND p.track_id=r.id;
    UPDATE publication_versions v SET content_hash=r.hash
        FROM pilot_rows r WHERE r.catalog=authority AND v.track_id=r.id AND v.revision=1;
    INSERT INTO publication_offers(track_id,revision,provider_node_id,expires_at)
        SELECT id,1,node,now()+interval '2 hours' FROM pilot_rows WHERE catalog=authority;
END;
$$;
SELECT format('CALL pg_temp.seed_origin(%L::uuid)',id) FROM pilot_origins ORDER BY s \gexec
ANALYZE tracks;
ANALYZE track_publications;
ANALYZE publication_versions;
ANALYZE publication_offers;
ANALYZE federation_search_documents;
SELECT json_build_object('virtual_catalogues',count(DISTINCT catalog),'virtual_tracks',count(*),
    'indexed_tracks',(SELECT count(*) FROM federation_search_documents d JOIN pilot_rows r ON r.id=d.track_id))
FROM pilot_rows;
