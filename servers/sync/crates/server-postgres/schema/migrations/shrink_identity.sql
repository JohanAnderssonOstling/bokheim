CREATE OR REPLACE FUNCTION enforce_lww_order() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    republished_seq bigint;
BEGIN
    IF (NEW.changed_at, NEW.version_rank, NEW.replica_id COLLATE "C", NEW.replica_seq)
       > (OLD.changed_at, OLD.version_rank, OLD.replica_id COLLATE "C", OLD.replica_seq)
    THEN
        RETURN NEW;
    END IF;
    -- The stored row already is this mutation, so a retried push has nothing
    -- to teach anyone. Cancel it rather than churn the channel head.
    IF (NEW.replica_id COLLATE "C", NEW.replica_seq) = (OLD.replica_id COLLATE "C", OLD.replica_seq) THEN
        RETURN NULL;
    END IF;
    -- The incoming mutation strictly loses. Its author applied it locally
    -- before pushing and is told only that the push was accepted -- the
    -- protocol has no "merged and discarded" acknowledgement -- so silently
    -- cancelling here would leave that replica permanently divergent: the
    -- winning row already sits at or below its pull cursor and would never be
    -- redelivered. Keep the stored winner, but move it to the sequence this
    -- statement already allocated so it lands above every replica's cursor and
    -- the losing writer converges on its next pull page.
    IF NEW.server_seq > OLD.server_seq THEN
        republished_seq := NEW.server_seq;
    ELSE
        republished_seq := nextval('sync_server_sequence');
    END IF;
    NEW := OLD;
    NEW.server_seq := republished_seq;
    RETURN NEW;
END;
$$;

DROP TRIGGER lww_guard ON sync_state;
ALTER TABLE sync_state DROP COLUMN event_id;
CREATE TRIGGER lww_guard BEFORE UPDATE OF library_id, kind, entity_key, entity_subkey, value, content_hash, changed_at, replica_id, replica_seq, version_rank ON sync_state FOR EACH ROW EXECUTE FUNCTION enforce_lww_order();
DROP TRIGGER lww_guard ON sync_reading_state;
ALTER TABLE sync_reading_state DROP COLUMN event_id;
CREATE TRIGGER lww_guard BEFORE UPDATE OF library_id, kind, entity_key, entity_subkey, value, content_hash, changed_at, replica_id, replica_seq, version_rank ON sync_reading_state FOR EACH ROW EXECUTE FUNCTION enforce_lww_order();
ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=3;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK (version=3);
