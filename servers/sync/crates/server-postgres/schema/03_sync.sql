-- Sync domain: one row per synchronized cell in a shared state table
-- (directories, books, placements, reading positions, annotations, metadata).
-- Depends only on 01_core.sql
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
-- resolution gets a `lww_guard` trigger (enforce_lww_order()): it refuses an
-- UPDATE whose (changed_at, version_rank, replica_id, replica_seq)
-- tuple does not beat the stored row's, so every writer -- this server, a
-- future service, a manual fix -- gets the same tie-break guarantee without
-- repeating the comparison in every upsert query. A refused update is not
-- simply cancelled: unless it is the stored row's own mutation arriving again,
-- the guard republishes the stored winner under the freshly allocated
-- server_seq, so the losing replica -- which applied the value locally and is
-- only ever told that its push was accepted -- receives the authoritative
-- value instead of diverging forever.
--
-- advance_library_sync_revision_statement() maintains sync_library_revision,
-- the per-library pull cursor, from whichever rows a statement actually
-- wrote (it fires on both INSERT and UPDATE, and does nothing for rows the
-- lww_guard trigger cancelled as a duplicate).
--
-- Cursor correctness requires every server_seq at or below an advertised
-- channel head to be committed. Every write path therefore locks its library
-- row with authorize_owned_library.sql before an INSERT or UPDATE can allocate
-- server_seq. Every exchange uses the same admission path and holds that lock
-- through commit. The following statements get a fresh READ COMMITTED snapshot
-- after waiting.
-- The revision statement trigger then advances the channel head while that
-- same transaction still owns the lock. Its sync_library_revision upsert is a
-- second serialization point, but it happens after sequence allocation and
-- cannot enforce this invariant by itself. Do not move or remove the pre-write
-- library lock without replacing it with a committed-watermark design and
-- exercising the cursor-gap test.

CREATE FUNCTION advance_library_sync_revision_statement() RETURNS trigger
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

-- Revision-only updates deliberately bypass the value guard: conflict repair
-- can redeliver an unchanged winner. Ordinary upserts still run the guard and
-- exact retries remain no-ops. Revision statement triggers cover both paths.
CREATE FUNCTION enforce_lww_order() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
    republished_seq bigint;
BEGIN
    -- Completed book additions are first-write-wins.
    -- A competing publication receives the accepted value below, even when
    -- its authored timestamp is newer.
    IF OLD.kind != 'book_add'
       AND (NEW.changed_at, NEW.version_rank, NEW.replica_id COLLATE "C", NEW.replica_seq)
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

CREATE SEQUENCE sync_server_sequence
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

CREATE TABLE sync_library_revision (
    library_id text PRIMARY KEY REFERENCES libraries(id) ON DELETE CASCADE,
    current_revision bigint NOT NULL CHECK (current_revision > 0)
);

-- Generic relay storage for every synchronized kind; the server treats `value` as opaque client-owned bytes.
-- `content_hash` names the book a value belongs to. A book_lifecycle row holds
-- that book's bytes for quota unless its version_rank is 2 (purge).
CREATE TABLE sync_state (
    library_id text NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    kind text NOT NULL,
    entity_key bytea NOT NULL,
    entity_subkey text NOT NULL DEFAULT '',
    value bytea NOT NULL,
    content_hash bytea,
    changed_at bigint NOT NULL,
    replica_id text NOT NULL,
    replica_seq bigint NOT NULL,
    version_rank smallint NOT NULL,
    server_seq bigint NOT NULL DEFAULT nextval('sync_server_sequence'::regclass),
    PRIMARY KEY (library_id, kind, entity_key, entity_subkey)
);
CREATE INDEX sync_state_library_seq ON sync_state (library_id, server_seq);
CREATE TRIGGER advance_library_sync_revision_state_insert AFTER INSERT ON sync_state REFERENCING NEW TABLE AS new_rows FOR EACH STATEMENT EXECUTE FUNCTION advance_library_sync_revision_statement();
CREATE TRIGGER advance_library_sync_revision_state_update AFTER UPDATE ON sync_state REFERENCING NEW TABLE AS new_rows FOR EACH STATEMENT EXECUTE FUNCTION advance_library_sync_revision_statement();
CREATE TRIGGER lww_guard BEFORE UPDATE OF library_id, kind, entity_key, entity_subkey, value, content_hash, changed_at, replica_id, replica_seq, version_rank ON sync_state FOR EACH ROW EXECUTE FUNCTION enforce_lww_order();
