DROP TRIGGER IF EXISTS trig_dir_added_insert;

DROP TRIGGER IF EXISTS trig_dir_added_reactivate;

DROP TRIGGER IF EXISTS trig_dir_moved;

DROP TRIGGER IF EXISTS trig_dir_deleted;

DROP TRIGGER IF EXISTS trig_dir_state_insert;

DROP TRIGGER IF EXISTS trig_dir_state_update;

DROP TRIGGER IF EXISTS trig_book_added;

DROP TRIGGER IF EXISTS trig_book_added_reactivate;

DROP TRIGGER IF EXISTS trig_book_deleted;

DROP TRIGGER IF EXISTS trig_book_dir_added_insert;

DROP TRIGGER IF EXISTS trig_book_dir_added_reactivate;

DROP TRIGGER IF EXISTS trig_book_dir_deleted;

DROP TRIGGER IF EXISTS trig_book_read_pos;

DROP TRIGGER IF EXISTS trig_book_last_read;

DROP TRIGGER IF EXISTS trig_book_metadata;

DROP TRIGGER IF EXISTS trig_event_log_hlc;

DROP TRIGGER IF EXISTS trig_event_log_id;

CREATE TRIGGER trig_dir_state_insert
AFTER INSERT ON dir
WHEN NEW.id != '__root__'
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO event_log (entity_id, event_type, payload, timestamp)
    VALUES (
        NEW.id,
        'dir_state',
        json_object(
            'parent_id', NEW.parent_id,
            'name', NEW.name,
            'deleted', NEW.deleted_at IS NOT NULL
        ),
        CAST(strftime('%s','now') || substr(strftime('%f','now'), 4) AS INTEGER)
    );
END;

CREATE TRIGGER trig_dir_state_update
AFTER UPDATE OF parent_id, name, deleted_at ON dir
WHEN NEW.id != '__root__'
  AND (
      OLD.parent_id IS NOT NEW.parent_id
      OR OLD.name IS NOT NEW.name
      OR OLD.deleted_at IS NOT NEW.deleted_at
  )
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO event_log (entity_id, event_type, payload, timestamp)
    VALUES (
        NEW.id,
        'dir_state',
        json_object(
            'parent_id', NEW.parent_id,
            'name', NEW.name,
            'deleted', NEW.deleted_at IS NOT NULL
        ),
        CAST(strftime('%s','now') || substr(strftime('%f','now'), 4) AS INTEGER)
    );
END;

CREATE TRIGGER trig_book_added
AFTER INSERT ON book
WHEN NEW.deleted_at IS NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO event_log (entity_id, event_type, payload, timestamp)
    SELECT
        NEW.id,
        'book_added',
        json_object(
            'title', NEW.title,
            'added_at', NEW.added_at
        ),
        ts
    FROM (
        SELECT CAST(strftime('%s','now') || substr(strftime('%f','now'), 4) AS INTEGER) AS ts
    );
END;

CREATE TRIGGER trig_book_added_reactivate
AFTER UPDATE OF deleted_at ON book
WHEN NEW.deleted_at IS NULL
  AND OLD.deleted_at IS NOT NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO event_log (entity_id, event_type, payload, timestamp)
    SELECT
        NEW.id,
        'book_added',
        json_object(
            'title', NEW.title,
            'added_at', NEW.added_at
        ),
        ts
    FROM (
        SELECT CAST(strftime('%s','now') || substr(strftime('%f','now'), 4) AS INTEGER) AS ts
    );
END;

CREATE TRIGGER trig_book_metadata
AFTER UPDATE OF title, author_id ON book
WHEN (
      OLD.title IS NOT NEW.title
      OR OLD.author_id IS NOT NEW.author_id
  )
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO event_log (entity_id, event_type, payload, timestamp)
    VALUES (
        NEW.id,
        'book_metadata_updated',
        json_object(
            'title', COALESCE(NEW.title, ''),
            'author', COALESCE((SELECT name FROM author WHERE id = NEW.author_id), '')
        ),
        CAST(strftime('%s','now') || substr(strftime('%f','now'), 4) AS INTEGER)
    );
END;

CREATE TRIGGER trig_book_deleted
AFTER UPDATE OF deleted_at ON book
WHEN NEW.deleted_at IS NOT NULL
  AND OLD.deleted_at IS NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO event_log (entity_id, event_type, payload, timestamp)
    SELECT
        NEW.id,
        'book_deleted',
        json_object(),
        ts
    FROM (
        SELECT CAST(strftime('%s','now') || substr(strftime('%f','now'), 4) AS INTEGER) AS ts
    );
END;

CREATE TRIGGER trig_book_dir_added_insert
AFTER INSERT ON book_dir
WHEN NEW.deleted_at IS NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO event_log (entity_id, event_type, payload, timestamp)
    VALUES (
        NEW.book_id,
        'book_dir_added',
        json_object(
            'dir_id', NEW.dir_id,
            'file_name', NEW.file_name,
            'content_hash', NEW.content_hash
        ),
        CAST(strftime('%s','now') || substr(strftime('%f','now'), 4) AS INTEGER)
    );
END;

CREATE TRIGGER trig_book_dir_added_reactivate
AFTER UPDATE OF deleted_at, dir_id, file_name, book_id, content_hash ON book_dir
WHEN NEW.deleted_at IS NULL
  AND (
      OLD.deleted_at IS NOT NULL
      OR OLD.dir_id IS NOT NEW.dir_id
      OR OLD.file_name IS NOT NEW.file_name
      OR OLD.book_id IS NOT NEW.book_id
      OR OLD.content_hash IS NOT NEW.content_hash
  )
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO event_log (entity_id, event_type, payload, timestamp)
    VALUES (
        NEW.book_id,
        'book_dir_added',
        json_object(
            'dir_id', NEW.dir_id,
            'file_name', NEW.file_name,
            'content_hash', NEW.content_hash
        ),
        CAST(strftime('%s','now') || substr(strftime('%f','now'), 4) AS INTEGER)
    );
END;

-- AFTER (not BEFORE) so trig_event_log_hlc's nested stamp of book_dir.hlc isn't
-- writing a row that's mid-update; by AFTER time the deleted_at write is settled.
CREATE TRIGGER trig_book_dir_deleted
AFTER UPDATE OF deleted_at ON book_dir
WHEN NEW.deleted_at IS NOT NULL
  AND OLD.deleted_at IS NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO event_log (entity_id, event_type, payload, timestamp)
    VALUES (
        OLD.book_id,
        'book_dir_removed',
        json_object(
            'dir_id', OLD.dir_id,
            'file_name', OLD.file_name
        ),
        CAST(strftime('%s','now') || substr(strftime('%f','now'), 4) AS INTEGER)
    );
END;

CREATE TRIGGER trig_book_read_pos
AFTER UPDATE OF read_pos ON book
WHEN NEW.read_pos IS NOT NULL
  AND COALESCE(OLD.read_pos, '') != COALESCE(NEW.read_pos, '')
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO event_log (entity_id, event_type, payload, timestamp)
    SELECT
        NEW.id,
        'reading_position_updated',
        json_object(
            'cfi', NEW.read_pos
        ),
        ts
    FROM (
        SELECT CAST(strftime('%s','now') || substr(strftime('%f','now'), 4) AS INTEGER) AS ts
    );
END;

CREATE TRIGGER trig_book_last_read
AFTER UPDATE OF last_read ON book
WHEN NEW.last_read > 0
  AND (OLD.last_read IS NULL OR OLD.last_read != NEW.last_read)
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO event_log (entity_id, event_type, payload, timestamp)
    SELECT
        NEW.id,
        'book_opened',
        json_object(
            'last_read', NEW.last_read
        ),
        ts
    FROM (
        SELECT CAST(strftime('%s','now') || substr(strftime('%f','now'), 4) AS INTEGER) AS ts
    );
END;

CREATE TRIGGER trig_event_log_hlc
AFTER INSERT ON event_log
BEGIN
    -- Upgraded databases have a constant empty-string default because SQLite
    -- cannot add a column with a random expression. Generate the identity in
    -- this same trigger so entity version stamping cannot race another trigger.
    UPDATE event_log
    SET event_id = lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-' ||
                   lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' ||
                   lower(hex(randomblob(6)))
    WHERE local_seq = NEW.local_seq AND event_id = '';

    UPDATE event_log
    SET
        hlc_pt = (
            SELECT
                CASE
                    WHEN prev_pt >= wall_now THEN prev_pt
                    ELSE wall_now
                END AS next_pt
            FROM (
                SELECT
                    CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) AS wall_now,
                    CAST(COALESCE((SELECT value FROM sync_metadata WHERE key = 'hlc_pt'), '0') AS INTEGER) AS prev_pt
            )
        ),
        hlc_l = (
            SELECT
                CASE
                    WHEN prev_pt >= wall_now THEN prev_l + 1
                    ELSE 0
                END AS next_l
            FROM (
                SELECT
                    CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER) AS wall_now,
                    CAST(COALESCE((SELECT value FROM sync_metadata WHERE key = 'hlc_pt'), '0') AS INTEGER) AS prev_pt,
                    CAST(COALESCE((SELECT value FROM sync_metadata WHERE key = 'hlc_l'), '0') AS INTEGER) AS prev_l
            )
        )
    WHERE local_seq = NEW.local_seq;

    INSERT INTO sync_metadata (key, value)
    SELECT 'hlc_pt', CAST(hlc_pt AS TEXT)
    FROM event_log
    WHERE local_seq = NEW.local_seq
    ON CONFLICT(key) DO UPDATE SET value = excluded.value;

    INSERT INTO sync_metadata (key, value)
    SELECT 'hlc_l', CAST(hlc_l AS TEXT)
    FROM event_log
    WHERE local_seq = NEW.local_seq
    ON CONFLICT(key) DO UPDATE SET value = excluded.value;

    -- Stamp lifecycle state with the exact HLC and actor-local sequence carried
    -- by its event. These writes do not touch event-producing columns.
    UPDATE book
    SET hlc_pt = (SELECT hlc_pt FROM event_log WHERE local_seq = NEW.local_seq),
        hlc_l  = (SELECT hlc_l  FROM event_log WHERE local_seq = NEW.local_seq),
        hlc_actor = COALESCE((SELECT value FROM sync_metadata WHERE key = 'client_id'), ''),
        hlc_seq = NEW.local_seq,
        hlc_event = (SELECT event_id FROM event_log WHERE local_seq = NEW.local_seq)
    WHERE NEW.event_type IN ('book_added', 'book_deleted')
      AND id = NEW.entity_id;

    UPDATE dir
    SET hlc_pt = (SELECT hlc_pt FROM event_log WHERE local_seq = NEW.local_seq),
        hlc_l  = (SELECT hlc_l  FROM event_log WHERE local_seq = NEW.local_seq),
        hlc_actor = COALESCE((SELECT value FROM sync_metadata WHERE key = 'client_id'), ''),
        hlc_seq = NEW.local_seq,
        hlc_event = (SELECT event_id FROM event_log WHERE local_seq = NEW.local_seq)
    WHERE NEW.event_type = 'dir_state'
      AND id = NEW.entity_id;

    -- Stamp the placement's LWW version using the same rule. No-op for other
    -- events; these columns do not re-fire the placement event triggers.
    UPDATE book_dir
    SET hlc_pt = (SELECT hlc_pt FROM event_log WHERE local_seq = NEW.local_seq),
        hlc_l  = (SELECT hlc_l  FROM event_log WHERE local_seq = NEW.local_seq),
        hlc_actor = COALESCE((SELECT value FROM sync_metadata WHERE key = 'client_id'), ''),
        hlc_seq = NEW.local_seq,
        hlc_event = (SELECT event_id FROM event_log WHERE local_seq = NEW.local_seq)
    WHERE NEW.event_type IN ('book_dir_added', 'book_dir_removed')
      AND book_id = NEW.entity_id
      AND dir_id = json_extract(NEW.payload, '$.dir_id')
      AND file_name = json_extract(NEW.payload, '$.file_name');
END;
