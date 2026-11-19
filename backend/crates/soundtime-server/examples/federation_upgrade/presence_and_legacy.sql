-- A v1 supplier vanishes, or is banned. Keep only an individually admitted
-- local copy. An independently withdrawn publication sharing its hash disables
-- ambiguous raw-hash playback even when the data is physically retained.
DO $$
DECLARE legacy uuid := '30000000-0000-4000-8000-000000000002'; source text := repeat('e',64);
        digest text := repeat('a',64); state text;
BEGIN
    UPDATE federation_catalogs SET node_id=repeat('b',64) WHERE is_local;
    INSERT INTO remote_tracks(id,local_track_id,title,artist_name,instance_domain,remote_uri,remote_stream_url)
    VALUES(gen_random_uuid(),legacy,'legacy','artist','p2p://'||source,'p2p://'||source||'/'||digest,'p2p://'||digest);
    INSERT INTO federation_peer_policies(node_id,is_followed) VALUES(source,true);
    INSERT INTO p2p_peers(node_id,is_online) VALUES(source,false);
    SELECT availability INTO STRICT state FROM federation_track_availability(true,true) WHERE track_id=legacy;
    IF state<>'temporarily_unavailable' THEN RAISE EXCEPTION 'offline v1 peer remains playable %',state; END IF;
    UPDATE p2p_peers SET is_online=true WHERE node_id=source;
    SELECT availability INTO STRICT state FROM federation_track_availability(true,true) WHERE track_id=legacy;
    IF state<>'playable' THEN RAISE EXCEPTION 'online v1 peer unavailable %',state; END IF;
    UPDATE federation_peer_policies SET is_followed=false WHERE node_id=source;
    SELECT availability INTO STRICT state FROM federation_track_availability(true,true) WHERE track_id=legacy;
    IF state<>'unavailable_on_instance' THEN RAISE EXCEPTION 'unfollowed v1 source accepted %',state; END IF;
    UPDATE federation_peer_policies SET is_banned=true WHERE node_id=source;
    INSERT INTO federation_legacy_copies VALUES(legacy,digest);
    SELECT availability INTO STRICT state FROM federation_track_availability(true,true) WHERE track_id=legacy;
    IF state<>'playable' THEN RAISE EXCEPTION 'retained bytes lost after ban %',state; END IF;
    UPDATE tracks SET content_hash=digest WHERE id='30000000-0000-4000-8000-000000000001';
    PERFORM federation_withdraw_local('30000000-0000-4000-8000-000000000001');
    SELECT availability INTO STRICT state FROM federation_track_availability(true,true) WHERE track_id=legacy;
    IF state='playable' THEN RAISE EXCEPTION 'retained raw copy bypassed a withdrawal'; END IF;
END;
$$;
