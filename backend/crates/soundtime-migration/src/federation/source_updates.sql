-- Source availability changes independently of the publication revision. Reuse
-- its frozen metadata so mirrors and pins of that revision stay valid.
CREATE TABLE federation_source_events (
    catalog_id uuid NOT NULL,
    cursor bigint NOT NULL,
    manifest_cursor bigint NOT NULL,
    present boolean NOT NULL,
    CHECK (cursor > manifest_cursor),
    PRIMARY KEY (catalog_id, cursor),
    FOREIGN KEY (catalog_id, manifest_cursor) REFERENCES federation_events(catalog_id, cursor)
);
CREATE TRIGGER fence_federation_writer BEFORE INSERT OR UPDATE OR DELETE ON federation_source_events
FOR EACH STATEMENT EXECUTE FUNCTION federation_check_writer();

CREATE FUNCTION federation_record_source_transition() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    target uuid;
    origin uuid;
    publication uuid;
    rev bigint;
    manifest_cursor bigint;
    next_cursor bigint;
BEGIN
    target := CASE WHEN TG_OP = 'DELETE' THEN OLD.track_id ELSE NEW.track_id END;
    SELECT p.catalog_id,p.origin_publication_id,p.revision INTO origin,publication,rev
      FROM track_publications p JOIN federation_catalogs c ON c.id=p.catalog_id AND c.is_local
      WHERE p.track_id=target AND p.state='active' FOR SHARE OF p;
    IF NOT FOUND THEN RETURN NULL; END IF;

    -- Serialize with federation_append_event and the initial journal bootstrap.
    SELECT change_cursor + 1 INTO STRICT next_cursor FROM federation_catalogs WHERE id=origin FOR UPDATE;
    SELECT e.cursor INTO manifest_cursor FROM federation_events e
      WHERE e.catalog_id=origin AND e.origin_publication_id=publication AND e.revision=rev AND e.kind='upsert'
        AND e.payload->>'content_hash' IS NOT NULL;
    -- Unpublished assets are first advertised using their actual availability.
    IF NOT FOUND THEN RETURN NULL; END IF;
    INSERT INTO federation_source_events(catalog_id,cursor,manifest_cursor,present)
      VALUES(origin,next_cursor,manifest_cursor,TG_OP='DELETE');
    UPDATE federation_catalogs SET change_cursor=next_cursor WHERE id=origin;
    RETURN NULL;
END;
$$;
CREATE TRIGGER federation_source_transition AFTER INSERT OR DELETE ON federation_missing_sources
FOR EACH ROW EXECUTE FUNCTION federation_record_source_transition();
