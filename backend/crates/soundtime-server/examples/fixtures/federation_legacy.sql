-- On the v34 schema. Stable IDs make preservation of individual relationships observable.
INSERT INTO users (id, username, email, password_hash) VALUES
    ('10000000-0000-4000-8000-000000000001', 'owner', 'owner@fixture.invalid', 'no-login'),
    ('10000000-0000-4000-8000-000000000002', 'listener', 'listener@fixture.invalid', 'no-login');
INSERT INTO artists (id, name) VALUES
    ('20000000-0000-4000-8000-000000000001', 'Historical Artist');

INSERT INTO tracks (id, title, artist_id, file_path, file_size, duration_secs, format, uploaded_by) VALUES
    ('30000000-0000-4000-8000-000000000001', 'Scanned without uploader', '20000000-0000-4000-8000-000000000001',
     'scanned/present.wav', 48, 0.0005, 'wav', NULL),
    ('30000000-0000-4000-8000-000000000002', 'Unconfirmed origin', '20000000-0000-4000-8000-000000000001',
     'p2p://' || repeat('a', 64), 2048, 1, 'flac', NULL),
    ('30000000-0000-4000-8000-000000000003', 'Temporarily missing', '20000000-0000-4000-8000-000000000001',
     'unmounted/missing.flac', 4096, 2, 'flac', '10000000-0000-4000-8000-000000000001');
UPDATE tracks SET content_hash = repeat('a', 64) WHERE id = '30000000-0000-4000-8000-000000000002';

-- Two old mentions of the same bytes. Neither proves who issued a publication.
INSERT INTO remote_tracks (id, local_track_id, title, artist_name, instance_domain, remote_uri, remote_stream_url) VALUES
    ('40000000-0000-4000-8000-000000000001', '30000000-0000-4000-8000-000000000002', 'Unconfirmed origin', 'Historical Artist',
     'legacy-a.test', 'https://legacy-a.test/audio/42', 'https://legacy-a.test/stream/42'),
    ('40000000-0000-4000-8000-000000000002', '30000000-0000-4000-8000-000000000002', 'Unconfirmed origin', 'Historical Artist',
     'legacy-c.test', 'https://legacy-c.test/audio/17', 'https://legacy-c.test/stream/17');

INSERT INTO playlists (id, name, user_id) VALUES
    ('50000000-0000-4000-8000-000000000001', 'Owner playlist', '10000000-0000-4000-8000-000000000001'),
    ('50000000-0000-4000-8000-000000000002', 'Listener playlist', '10000000-0000-4000-8000-000000000002');
INSERT INTO playlist_tracks (playlist_id, position, track_id) VALUES
    ('50000000-0000-4000-8000-000000000001', 0, '30000000-0000-4000-8000-000000000001'),
    ('50000000-0000-4000-8000-000000000001', 1, '30000000-0000-4000-8000-000000000002'),
    ('50000000-0000-4000-8000-000000000001', 2, '30000000-0000-4000-8000-000000000003'),
    ('50000000-0000-4000-8000-000000000002', 0, '30000000-0000-4000-8000-000000000003'),
    ('50000000-0000-4000-8000-000000000002', 1, '30000000-0000-4000-8000-000000000002'),
    ('50000000-0000-4000-8000-000000000002', 2, '30000000-0000-4000-8000-000000000001');
INSERT INTO favorites (user_id, track_id) VALUES
    ('10000000-0000-4000-8000-000000000001', '30000000-0000-4000-8000-000000000002'),
    ('10000000-0000-4000-8000-000000000002', '30000000-0000-4000-8000-000000000003');
INSERT INTO libraries (id, name, user_id, total_tracks) VALUES
    ('60000000-0000-4000-8000-000000000001', 'Historical collection', '10000000-0000-4000-8000-000000000002', 2);
INSERT INTO library_tracks (library_id, track_id) VALUES
    ('60000000-0000-4000-8000-000000000001', '30000000-0000-4000-8000-000000000001'),
    ('60000000-0000-4000-8000-000000000001', '30000000-0000-4000-8000-000000000002');
INSERT INTO listen_history (id, user_id, track_id, listened_at, duration_listened) VALUES
    ('70000000-0000-4000-8000-000000000001', '10000000-0000-4000-8000-000000000001', '30000000-0000-4000-8000-000000000002', '2024-05-01T10:00:00Z', 1),
    ('70000000-0000-4000-8000-000000000002', '10000000-0000-4000-8000-000000000002', '30000000-0000-4000-8000-000000000003', '2024-05-02T10:00:00Z', 2);
