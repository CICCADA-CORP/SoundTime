CREATE TEMP TABLE proof_rows ON COMMIT DROP AS
SELECT lpad(to_hex(n),32,'0')::uuid AS id,md5('proof-source-'||n)::uuid AS catalog,
       'proof-source-'||n AS node,repeat(CASE n WHEN 101 THEN 'a' ELSE 'b' END,64) AS hash
FROM generate_series(101,102) n;
INSERT INTO federation_catalogs(id,node_id,is_local) SELECT catalog,node,false FROM proof_rows;
INSERT INTO federation_peer_policies(node_id,is_followed) SELECT node,true FROM proof_rows;
INSERT INTO tracks(id,title,artist_id,file_path,file_size,duration_secs,format,content_hash)
SELECT id,'Prooffixture','10000000-0000-0000-0000-000000000001','p2p://'||hash,4,1,'wav',hash FROM proof_rows;
INSERT INTO federation_assets(content_hash,file_size) SELECT hash,4 FROM proof_rows;
UPDATE track_publications p SET state='active',catalog_id=r.catalog,origin_publication_id=r.id
FROM proof_rows r WHERE r.id=p.track_id;
UPDATE publication_versions v SET content_hash=r.hash FROM proof_rows r WHERE r.id=v.track_id AND v.revision=1;
INSERT INTO publication_offers(track_id,revision,provider_node_id,expires_at)
SELECT id,1,node,now()+interval '1 hour' FROM proof_rows;
