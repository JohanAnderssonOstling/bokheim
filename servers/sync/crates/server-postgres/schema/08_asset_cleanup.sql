-- Append-only work avoids contending on one queue row when different accounts
-- release the same book. Workers deduplicate batches and acknowledge exact IDs,
-- so a concurrent enqueue cannot be lost during filesystem collection.
CREATE TABLE asset_cleanup_queue (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind text NOT NULL CHECK(kind IN ('book', 'catalog', 'thumbnail')),
    owner text NOT NULL DEFAULT '',
    content_hash bytea NOT NULL
);

CREATE FUNCTION queue_released_books() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO asset_cleanup_queue(kind, content_hash)
    SELECT DISTINCT 'book', content_hash FROM released_rows;
    RETURN NULL;
END;
$$;
CREATE TRIGGER queue_released_charges AFTER DELETE ON user_blob_charge
    REFERENCING OLD TABLE AS released_rows FOR EACH STATEMENT EXECUTE FUNCTION queue_released_books();
CREATE TRIGGER queue_released_claims AFTER DELETE ON library_blob_claim
    REFERENCING OLD TABLE AS released_rows FOR EACH STATEMENT EXECUTE FUNCTION queue_released_books();

CREATE FUNCTION queue_released_canonical_books() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO asset_cleanup_queue(kind, content_hash)
    SELECT DISTINCT 'book', byte_checksum FROM released_rows;
    RETURN NULL;
END;
$$;
CREATE TRIGGER queue_released_canonical_charges AFTER DELETE ON user_canonical_charge
    REFERENCING OLD TABLE AS released_rows FOR EACH STATEMENT EXECUTE FUNCTION queue_released_canonical_books();

CREATE FUNCTION queue_deleted_lifecycles() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO asset_cleanup_queue(kind, owner, content_hash)
    SELECT DISTINCT 'thumbnail', library.user_id, prior_state.content_hash
    FROM old_states prior_state JOIN libraries library ON library.id=prior_state.library_id
    WHERE prior_state.kind='book_lifecycle' AND prior_state.version_rank<>2 AND prior_state.content_hash IS NOT NULL;
    RETURN NULL;
END;
$$;
CREATE TRIGGER queue_deleted_lifecycles AFTER DELETE ON sync_state
    REFERENCING OLD TABLE AS old_states FOR EACH STATEMENT EXECUTE FUNCTION queue_deleted_lifecycles();

CREATE FUNCTION queue_retired_lifecycles() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO asset_cleanup_queue(kind, owner, content_hash)
    SELECT DISTINCT 'thumbnail', library.user_id, prior_state.content_hash
    FROM old_states prior_state
    JOIN new_states next_state USING(library_id, kind, entity_key, entity_subkey)
    JOIN libraries library ON library.id=prior_state.library_id
    WHERE prior_state.kind='book_lifecycle' AND prior_state.version_rank<>2 AND prior_state.content_hash IS NOT NULL
      AND (next_state.version_rank=2 OR next_state.content_hash IS DISTINCT FROM prior_state.content_hash);
    RETURN NULL;
END;
$$;
CREATE TRIGGER queue_retired_lifecycles AFTER UPDATE ON sync_state
    REFERENCING OLD TABLE AS old_states NEW TABLE AS new_states
    FOR EACH STATEMENT EXECUTE FUNCTION queue_retired_lifecycles();

-- During an account cascade the library is already invisible to the state
-- DELETE trigger. Capture the cover owners before the library disappears.
CREATE FUNCTION queue_deleted_library_covers() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO asset_cleanup_queue(kind, owner, content_hash)
    SELECT DISTINCT 'thumbnail', OLD.user_id, content_hash FROM sync_state
    WHERE library_id=OLD.id AND kind='book_lifecycle' AND version_rank<>2 AND content_hash IS NOT NULL;
    RETURN OLD;
END;
$$;
CREATE TRIGGER queue_deleted_library_covers BEFORE DELETE ON libraries
    FOR EACH ROW EXECUTE FUNCTION queue_deleted_library_covers();
