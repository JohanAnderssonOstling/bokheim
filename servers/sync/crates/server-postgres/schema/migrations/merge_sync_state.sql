-- Preserve each authored version and delivery revision. No triggers run while
-- moving rows, so an upgrade neither allocates revisions nor republishes state.
DROP TRIGGER advance_library_sync_revision_state_insert ON sync_state;
INSERT INTO sync_state(library_id,kind,entity_key,entity_subkey,value,content_hash,changed_at,replica_id,replica_seq,version_rank,server_seq)
SELECT library_id,kind,entity_key,entity_subkey,value,content_hash,changed_at,replica_id,replica_seq,version_rank,server_seq FROM sync_reading_state;
DROP TABLE sync_reading_state;
CREATE OR REPLACE FUNCTION advance_library_sync_revision_statement() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- INSERT ... ON CONFLICT DO UPDATE can invoke both statement trigger
    -- families. Each transition table contains only rows handled by that
    -- operation, so an empty half performs no revision write and a mixed half
    -- advances the head at most once per affected library.
    INSERT INTO sync_library_revision(library_id, current_revision)
    SELECT library_id, MAX(server_seq) FROM new_rows GROUP BY library_id
    ON CONFLICT (library_id) DO UPDATE SET
        current_revision=GREATEST(sync_library_revision.current_revision, EXCLUDED.current_revision);
    RETURN NULL;
END
$$;

CREATE TRIGGER advance_library_sync_revision_state_insert AFTER INSERT ON sync_state REFERENCING NEW TABLE AS new_rows FOR EACH STATEMENT EXECUTE FUNCTION advance_library_sync_revision_statement();
ALTER TABLE sync_library_revision DROP COLUMN reading_revision, DROP COLUMN other_revision;
ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=4;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK (version=4);
