-- This transaction is rolled back after EXPLAIN and the policy checks.
CREATE TEMP TABLE search_fixture AS
SELECT i, md5('search track ' || i)::uuid AS id, md5('search issuer ' || (i%300))::uuid AS catalog,
       lpad(to_hex(i%300+1),64,'0') AS provider, lpad(to_hex(i),64,'0') AS hash
FROM generate_series(1,3000) i;
INSERT INTO federation_catalogs(id,node_id)
SELECT DISTINCT catalog, provider FROM search_fixture;
INSERT INTO federation_peer_policies(node_id,is_followed)
SELECT DISTINCT provider,true FROM search_fixture;
INSERT INTO artists(id,name) VALUES ('22000000-0000-4000-8000-000000000001','Amberartist');
INSERT INTO albums(id,title,artist_id) VALUES
('22000000-0000-4000-8000-000000000002','Cobaltalbum','22000000-0000-4000-8000-000000000001');
INSERT INTO tracks(id,title,artist_id,album_id,file_path,file_size,duration_secs,format,content_hash)
SELECT id, CASE WHEN i=1 THEN 'Needleuniquefixture' ELSE 'Commonfixture' END,
       '22000000-0000-4000-8000-000000000001','22000000-0000-4000-8000-000000000002',
       'p2p://' || hash,1024,1,'wav',hash FROM search_fixture;
UPDATE track_publications p SET catalog_id=f.catalog,origin_publication_id=f.id,state='active'
FROM search_fixture f WHERE f.id=p.track_id;
INSERT INTO federation_assets(content_hash,file_size) SELECT hash,1024 FROM search_fixture;
UPDATE publication_versions v SET content_hash=f.hash FROM search_fixture f WHERE v.track_id=f.id;
INSERT INTO publication_offers(track_id,revision,provider_node_id,expires_at)
SELECT id,1,provider,now()+interval '1 day' FROM search_fixture;
ANALYZE federation_search_documents;
ANALYZE tracks;
ANALYZE track_publications;
ANALYZE publication_offers;
ANALYZE publication_versions;
ANALYZE federation_catalogs;
ANALYZE federation_peer_policies;
