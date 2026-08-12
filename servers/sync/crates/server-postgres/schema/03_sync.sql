-- Sync domain: one row-per-logical-update state table per entity kind
-- (directories, books, placements, reading positions, annotations,
-- bookmarks, book links, metadata). Depends only on 01_core.sql
-- (libraries).
--
-- The server stores and relays synchronized state; it does not interpret
-- it. Directories and placements keep a handful of typed columns because
-- the server itself needs them (existence checks for referential
-- dependencies, and content-hash deltas computed transactionally for blob
-- quota accounting). Every other domain's payload is an opaque
-- `value bytea` -- the wire-encoded client type -- so adding or changing a
-- synchronized field never touches this schema; only the client
-- interprets what is inside.
--
-- Every table below that participates in last-writer-wins conflict
-- resolution gets a `lww_guard` trigger (enforce_lww_order()): it silently
-- cancels an UPDATE whose (changed_at, version_rank, replica_id, replica_seq,
-- event_id) tuple does not beat the stored row's, so every writer -- this
-- server, a future service, a manual fix -- gets the same tie-break
-- guarantee without repeating the comparison in every upsert query.
--
-- advance_library_sync_revision_statement() maintains sync_library_revision,
-- the per-library pull cursor, from whichever rows a statement actually
-- wrote (it fires on both INSERT and UPDATE, and does nothing for rows the
-- lww_guard trigger already cancelled).

CREATE FUNCTION advance_library_sync_revision_statement() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    -- INSERT ... ON CONFLICT DO UPDATE can invoke both statement trigger
    -- families. Each transition table contains only rows handled by that
    -- operation, so an empty half performs no revision write and a mixed half
    -- advances the head at most once per affected library.
    IF TG_TABLE_NAME = 'sync_reading_state' THEN
        INSERT INTO sync_library_revision(library_id, current_revision, reading_revision, other_revision)
        SELECT library_id, MAX(server_seq), MAX(server_seq), 0
        FROM new_rows GROUP BY library_id
        ON CONFLICT (library_id) DO UPDATE SET
            current_revision=GREATEST(sync_library_revision.current_revision, EXCLUDED.current_revision),
            reading_revision=GREATEST(sync_library_revision.reading_revision, EXCLUDED.reading_revision),
            other_revision=GREATEST(sync_library_revision.other_revision, EXCLUDED.other_revision);
    ELSE
        INSERT INTO sync_library_revision(library_id, current_revision, reading_revision, other_revision)
        SELECT library_id, MAX(server_seq), 0, MAX(server_seq)
        FROM new_rows GROUP BY library_id
        ON CONFLICT (library_id) DO UPDATE SET
            current_revision=GREATEST(sync_library_revision.current_revision, EXCLUDED.current_revision),
            reading_revision=GREATEST(sync_library_revision.reading_revision, EXCLUDED.reading_revision),
            other_revision=GREATEST(sync_library_revision.other_revision, EXCLUDED.other_revision);
    END IF;
    RETURN NULL;
END
$$;

CREATE FUNCTION enforce_lww_order() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF (NEW.changed_at, NEW.version_rank, NEW.replica_id COLLATE "C", NEW.replica_seq, NEW.event_id COLLATE "C")
       > (OLD.changed_at, OLD.version_rank, OLD.replica_id COLLATE "C", OLD.replica_seq, OLD.event_id COLLATE "C")
    THEN
        RETURN NEW;
    END IF;
    RETURN NULL;
END;
$$;

CREATE SEQUENCE sync_server_sequence
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

CREATE TABLE sync_library_revision (
    library_id text PRIMARY KEY REFERENCES libraries(id) ON DELETE CASCADE,
    current_revision bigint NOT NULL CHECK (current_revision > 0),
    reading_revision bigint NOT NULL DEFAULT 0,
    other_revision bigint NOT NULL DEFAULT 0,
    CONSTRAINT sync_library_revision_complete_channels CHECK (current_revision = GREATEST(reading_revision, other_revision)),
    CONSTRAINT sync_library_revision_nonnegative_channels CHECK (reading_revision >= 0 AND other_revision >= 0)
);

-- Generic relay storage. Cold and high-frequency reading state share this
-- structure; the server treats `value` as opaque client-owned bytes.
CREATE TABLE sync_state (
    library_id text NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    kind text NOT NULL,
    entity_key text NOT NULL,
    entity_subkey text NOT NULL DEFAULT '',
    value bytea NOT NULL,
    present boolean,
    content_hash text,
    changed_at bigint NOT NULL,
    replica_id text NOT NULL,
    replica_seq bigint NOT NULL,
    version_rank smallint NOT NULL,
    event_id text NOT NULL,
    server_seq bigint NOT NULL DEFAULT nextval('sync_server_sequence'::regclass),
    PRIMARY KEY (library_id, kind, entity_key, entity_subkey),
    UNIQUE (library_id, event_id)
);
CREATE INDEX sync_state_library_seq ON sync_state (library_id, server_seq);
CREATE TRIGGER advance_library_sync_revision_state_insert AFTER INSERT ON sync_state REFERENCING NEW TABLE AS new_rows FOR EACH STATEMENT EXECUTE FUNCTION advance_library_sync_revision_statement();
CREATE TRIGGER advance_library_sync_revision_state_update AFTER UPDATE ON sync_state REFERENCING NEW TABLE AS new_rows FOR EACH STATEMENT EXECUTE FUNCTION advance_library_sync_revision_statement();
CREATE TRIGGER lww_guard BEFORE UPDATE ON sync_state FOR EACH ROW EXECUTE FUNCTION enforce_lww_order();

CREATE TABLE sync_reading_state (
    library_id text NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    kind text NOT NULL,
    entity_key text NOT NULL,
    entity_subkey text NOT NULL DEFAULT '',
    value bytea NOT NULL,
    present boolean,
    content_hash text,
    changed_at bigint NOT NULL,
    replica_id text NOT NULL,
    replica_seq bigint NOT NULL,
    version_rank smallint NOT NULL,
    event_id text NOT NULL,
    server_seq bigint NOT NULL DEFAULT nextval('sync_server_sequence'::regclass),
    PRIMARY KEY (library_id, kind, entity_key, entity_subkey),
    UNIQUE (library_id, event_id)
);
CREATE INDEX sync_reading_state_library_seq ON sync_reading_state (library_id, server_seq);
CREATE TRIGGER advance_library_sync_revision_reading_insert AFTER INSERT ON sync_reading_state REFERENCING NEW TABLE AS new_rows FOR EACH STATEMENT EXECUTE FUNCTION advance_library_sync_revision_statement();
CREATE TRIGGER advance_library_sync_revision_reading_update AFTER UPDATE ON sync_reading_state REFERENCING NEW TABLE AS new_rows FOR EACH STATEMENT EXECUTE FUNCTION advance_library_sync_revision_statement();
CREATE TRIGGER lww_guard BEFORE UPDATE ON sync_reading_state FOR EACH ROW EXECUTE FUNCTION enforce_lww_order();
