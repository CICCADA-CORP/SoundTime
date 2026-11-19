-- All rows live inside the migration fixture's rolled-back transaction.
DO $$
DECLARE
    missing uuid := gen_random_uuid();
    literal uuid := gen_random_uuid();
    song uuid := gen_random_uuid();
    remote_missing uuid;
    remote_literal uuid;
    manifest jsonb := jsonb_build_object('artist',' ', 'title','Creditfixture', 'format','wav',
                                         'hash',repeat('d2',32), 'size',31, 'duration_ms',1000);
    original jsonb;
    revised jsonb;
BEGIN
    IF NOT EXISTS (SELECT FROM artists WHERE id='20000000-0000-4000-8000-000000000002'
                   AND name='Unknown Artist' AND NOT is_placeholder)
       OR NOT EXISTS (SELECT FROM artists WHERE id='20000000-0000-4000-8000-000000000003' AND is_placeholder)
       THEN RAISE EXCEPTION 'ambiguous v34 credit was reclassified or a blank name remained visible'; END IF;
    INSERT INTO artists(id,name) VALUES(missing,'Unknown Artist'),(literal,'Unknown Artist');
    INSERT INTO tracks(id,title,artist_id,file_path,file_size,duration_secs,format,content_hash)
    VALUES(song,'Creditfixture',missing,'credit/source.wav',31,1,'wav',repeat('d2',32));
    SELECT payload INTO STRICT original FROM federation_events WHERE origin_publication_id=song AND revision=1;
    IF original->>'artist_name' IS DISTINCT FROM 'Unknown Artist' THEN RAISE EXCEPTION 'lost genuine credit'; END IF;
    UPDATE artists SET is_placeholder=true WHERE id=missing;
    WHILE EXISTS(SELECT FROM federation_search_jobs WHERE subject_id=missing) LOOP
        PERFORM federation_refresh_search_batch(128);
    END LOOP;
    SELECT payload INTO STRICT revised FROM federation_events WHERE origin_publication_id=song AND revision=2;
    IF revised->>'artist_name' IS DISTINCT FROM ' ' OR NOT EXISTS(
        SELECT FROM federation_events WHERE origin_publication_id=song AND revision=1 AND payload=original
    ) THEN RAISE EXCEPTION 'missing-credit migration changed an earlier signed payload'; END IF;
    IF EXISTS(SELECT FROM federation_search_input WHERE track_id=song AND document @@ plainto_tsquery('english','Unknown Artist'))
       THEN RAISE EXCEPTION 'placeholder leaked into search'; END IF;
    remote_missing := federation_import_signed('credit-fixture',gen_random_uuid(),1,manifest,manifest);
    manifest := jsonb_set(manifest,'{artist}',to_jsonb('Unknown Artist'::text));
    remote_literal := federation_import_signed('credit-fixture',gen_random_uuid(),1,manifest,manifest);
    IF NOT EXISTS(SELECT FROM tracks t JOIN artists a ON a.id=t.artist_id
        WHERE t.id=remote_missing AND a.is_placeholder AND a.name='Unknown Artist')
       OR NOT EXISTS(SELECT FROM tracks t JOIN artists a ON a.id=t.artist_id
        WHERE t.id=remote_literal AND NOT a.is_placeholder AND a.name='Unknown Artist')
       OR NOT EXISTS(SELECT FROM federation_search_input
        WHERE track_id=remote_literal AND document @@ plainto_tsquery('english','Unknown Artist'))
       THEN RAISE EXCEPTION 'remote missing credit and homonym were merged'; END IF;
END;
$$;
