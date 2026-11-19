INSERT INTO p2p_peers(node_id,name,version,track_count,is_online,last_seen_at,presence_checked_at)
SELECT p.id,p.name,p.version,p.tracks,p.online,p.seen,p.checked
FROM jsonb_to_recordset($1) AS p(id text,name text,version text,tracks bigint,online boolean,seen timestamptz,checked timestamptz)
ON CONFLICT(node_id) DO UPDATE SET
    name=coalesce(excluded.name,p2p_peers.name), version=coalesce(excluded.version,p2p_peers.version),
    track_count=excluded.track_count, is_online=excluded.is_online,
    last_seen_at=greatest(p2p_peers.last_seen_at,excluded.last_seen_at), presence_checked_at=excluded.presence_checked_at
WHERE excluded.presence_checked_at >= p2p_peers.presence_checked_at
