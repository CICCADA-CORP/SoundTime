CREATE TABLE federation_worker_lease (
    singleton boolean PRIMARY KEY CHECK(singleton),
    holder uuid NOT NULL,
    generation bigint NOT NULL CHECK(generation > 0),
    expires_at timestamptz NOT NULL,
    UNIQUE(holder,generation)
);

-- Database time orders takeovers. A new process always uses a new holder UUID.
CREATE FUNCTION federation_claim_worker(candidate uuid) RETURNS bigint LANGUAGE sql AS $$
    INSERT INTO federation_worker_lease AS l(singleton,holder,generation,expires_at)
    VALUES(true,candidate,1,clock_timestamp()+interval '15 seconds')
    ON CONFLICT(singleton) DO UPDATE SET holder=excluded.holder,generation=l.generation+1,
        expires_at=clock_timestamp()+interval '15 seconds' WHERE l.expires_at<=clock_timestamp()
    RETURNING generation
$$;

-- A takeover waits for previously admitted writes to commit; later writes from
-- the old generation are rejected. Standalone databases have no lease row.
CREATE FUNCTION federation_check_writer() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE current_lease federation_worker_lease%ROWTYPE;
BEGIN
    SELECT * INTO current_lease FROM federation_worker_lease WHERE singleton FOR KEY SHARE;
    IF FOUND AND (current_setting('application_name') <> 'st/' || current_lease.generation || '/' || current_lease.holder
                  OR current_lease.expires_at <= clock_timestamp()) THEN
        RAISE EXCEPTION 'inactive federation writer' USING ERRCODE='55000';
    END IF;
    RETURN NULL;
END;
$$;
DO $$
DECLARE relation text;
BEGIN
    FOREACH relation IN ARRAY ARRAY['track_publications','publication_versions','publication_offers','federation_events',
        'federation_inventories','federation_cached_publications','federation_mirror_leases'] LOOP
        EXECUTE format('CREATE TRIGGER fence_federation_writer BEFORE INSERT OR UPDATE OR DELETE ON %I
            FOR EACH STATEMENT EXECUTE FUNCTION federation_check_writer()',relation);
    END LOOP;
END;
$$;

-- NOTIFY is delivered only upon commit. Hash requests require invalidation for
-- every known version of a withdrawn publication, independently of mirror leases.
CREATE FUNCTION federation_notify_policy() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE affected uuid; hash text;
BEGIN
    IF TG_TABLE_NAME IN ('track_publications','federation_hidden_publications') THEN
        affected := NEW.track_id;
        PERFORM pg_notify('soundtime_policy','track:' || affected);
        FOR hash IN SELECT content_hash FROM publication_versions WHERE track_id=affected AND content_hash IS NOT NULL
                    UNION SELECT content_hash FROM tracks WHERE tracks.id=affected AND content_hash IS NOT NULL LOOP
            PERFORM pg_notify('soundtime_policy','raw:' || hash);
        END LOOP;
    ELSIF TG_TABLE_NAME='federation_content_blocks' THEN
        PERFORM pg_notify('soundtime_policy','block:' || NEW.content_hash);
    ELSIF TG_TABLE_NAME='blocked_domains' THEN
        PERFORM pg_notify('soundtime_policy','peer:' || left(NEW.domain,256));
    ELSE
        PERFORM pg_notify('soundtime_policy','peer:' || NEW.node_id);
    END IF;
    RETURN NULL;
END;
$$;
CREATE TRIGGER notify_withdrawal AFTER UPDATE OF state ON track_publications FOR EACH ROW
    WHEN(NEW.state='withdrawn' AND OLD.state IS DISTINCT FROM NEW.state) EXECUTE FUNCTION federation_notify_policy();
CREATE TRIGGER notify_hidden AFTER INSERT ON federation_hidden_publications FOR EACH ROW EXECUTE FUNCTION federation_notify_policy();
CREATE TRIGGER notify_hash_block AFTER INSERT ON federation_content_blocks FOR EACH ROW EXECUTE FUNCTION federation_notify_policy();
CREATE TRIGGER notify_blocked_peer AFTER INSERT ON blocked_domains FOR EACH ROW EXECUTE FUNCTION federation_notify_policy();
CREATE TRIGGER notify_peer_policy AFTER UPDATE ON federation_peer_policies FOR EACH ROW
    WHEN((NEW.is_banned AND NOT OLD.is_banned) OR (OLD.is_followed AND NOT NEW.is_followed)) EXECUTE FUNCTION federation_notify_policy();
