-- Deterministic ordering distinguishes a moved, lost or silently replaced playlist entry.
SELECT jsonb_build_object(
    'tracks', (SELECT jsonb_agg(jsonb_build_array(id, title, artist_id, uploaded_by) ORDER BY id) FROM tracks),
    'playlists', (SELECT jsonb_agg(jsonb_build_array(id, name, user_id) ORDER BY id) FROM playlists),
    'positions', (SELECT jsonb_agg(jsonb_build_array(playlist_id, position, track_id) ORDER BY playlist_id, position, track_id) FROM playlist_tracks),
    'favorites', (SELECT jsonb_agg(jsonb_build_array(user_id, track_id) ORDER BY user_id, track_id) FROM favorites),
    'libraries', (SELECT jsonb_agg(jsonb_build_array(id, name, user_id) ORDER BY id) FROM libraries),
    'memberships', (SELECT jsonb_agg(jsonb_build_array(library_id, track_id) ORDER BY library_id, track_id) FROM library_tracks),
    'listens', (SELECT jsonb_agg(jsonb_build_array(id, user_id, track_id, listened_at, duration_listened) ORDER BY id) FROM listen_history)
) AS snapshot;
