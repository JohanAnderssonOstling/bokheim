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
DROP TRIGGER IF EXISTS trig_annotation_state_insert;
DROP TRIGGER IF EXISTS trig_annotation_state_update;
DROP TRIGGER IF EXISTS trig_bookmark_state_insert;
DROP TRIGGER IF EXISTS trig_bookmark_state_update;
DROP TRIGGER IF EXISTS trig_book_link_state_insert;
DROP TRIGGER IF EXISTS trig_book_link_state_update;
DROP TRIGGER IF EXISTS trig_sync_outbox_stamp_insert;
DROP TRIGGER IF EXISTS trig_sync_outbox_stamp_update;

-- Every producer passes typed SQL values through the registered Rust encoder.
-- Invalid protocol values abort the surrounding transaction; valid values are
-- stored as versioned bincode without an intermediate JSON representation.
CREATE TRIGGER trig_dir_state_insert
AFTER INSERT ON dir
WHEN NEW.id != '__root__'
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'directory', NEW.id,
        bokheim_mutation('directory', NEW.id, NEW.deleted_at IS NOT NULL, NEW.parent_id, NEW.name),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.deleted_at IS NOT NULL
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_dir_state_update
AFTER UPDATE OF parent_id, name, deleted_at ON dir
WHEN NEW.id != '__root__'
  AND (OLD.parent_id IS NOT NEW.parent_id OR OLD.name IS NOT NEW.name OR OLD.deleted_at IS NOT NEW.deleted_at)
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'directory', NEW.id,
        bokheim_mutation('directory', NEW.id, NEW.deleted_at IS NOT NULL, NEW.parent_id, NEW.name),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.deleted_at IS NOT NULL
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_added
AFTER INSERT ON book
WHEN NEW.deleted_at IS NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at)
    VALUES (
        'book_lifecycle', NEW.id,
        bokheim_mutation('book_lifecycle', NEW.id, 0, COALESCE(NEW.title, NEW.id), NEW.authors, COALESCE(NEW.description, ''), NEW.publication_metadata, COALESCE(NEW.added_at, 0)),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at;
END;

CREATE TRIGGER trig_book_added_reactivate
AFTER UPDATE OF deleted_at ON book
WHEN NEW.deleted_at IS NULL AND OLD.deleted_at IS NOT NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at)
    VALUES (
        'book_lifecycle', NEW.id,
        bokheim_mutation('book_lifecycle', NEW.id, 0, COALESCE(NEW.title, NEW.id), NEW.authors, COALESCE(NEW.description, ''), NEW.publication_metadata, COALESCE(NEW.added_at, 0)),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at;
END;

CREATE TRIGGER trig_book_deleted
AFTER UPDATE OF deleted_at ON book
WHEN NEW.deleted_at IS NOT NULL AND OLD.deleted_at IS NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at)
    VALUES (
        'book_lifecycle', NEW.id,
        bokheim_mutation('book_lifecycle', NEW.id, 1, '', X'', '', X'', 0),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at;
END;

CREATE TRIGGER trig_book_metadata
AFTER UPDATE OF title, authors, description, publication_metadata ON book
WHEN (OLD.title IS NOT NEW.title OR OLD.authors IS NOT NEW.authors OR OLD.description IS NOT NEW.description OR OLD.publication_metadata IS NOT NEW.publication_metadata)
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at)
    VALUES (
        'metadata', NEW.id,
        bokheim_mutation('metadata', NEW.id, COALESCE(NEW.title, ''), NEW.authors, COALESCE(NEW.description, ''), NEW.publication_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at;
END;

CREATE TRIGGER trig_book_dir_added_insert
AFTER INSERT ON book_dir
WHEN NEW.deleted_at IS NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, placement_dir_id, placement_file_name, placement_book_id)
    VALUES (
        'placement', lower(hex(randomblob(16))),
        bokheim_mutation('placement', NEW.dir_id, NEW.file_name, 1, NEW.book_id, NEW.content_hash),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.dir_id, NEW.file_name, NEW.book_id
    );
END;

CREATE TRIGGER trig_book_dir_added_reactivate
AFTER UPDATE OF deleted_at, dir_id, file_name, book_id, content_hash ON book_dir
WHEN NEW.deleted_at IS NULL
  AND (OLD.deleted_at IS NOT NULL OR OLD.dir_id IS NOT NEW.dir_id OR OLD.file_name IS NOT NEW.file_name OR OLD.book_id IS NOT NEW.book_id OR OLD.content_hash IS NOT NEW.content_hash)
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, placement_dir_id, placement_file_name, placement_book_id)
    VALUES (
        'placement', lower(hex(randomblob(16))),
        bokheim_mutation('placement', NEW.dir_id, NEW.file_name, 1, NEW.book_id, NEW.content_hash),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.dir_id, NEW.file_name, NEW.book_id
    );
END;

CREATE TRIGGER trig_book_dir_deleted
AFTER UPDATE OF deleted_at ON book_dir
WHEN NEW.deleted_at IS NOT NULL AND OLD.deleted_at IS NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, placement_dir_id, placement_file_name, placement_book_id)
    VALUES (
        'placement', lower(hex(randomblob(16))),
        bokheim_mutation('placement', OLD.dir_id, OLD.file_name, 0, OLD.book_id, NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), OLD.dir_id, OLD.file_name, OLD.book_id
    );
END;

CREATE TRIGGER trig_book_read_pos
AFTER UPDATE OF read_pos, read_progress ON book
WHEN NEW.read_pos IS NOT NULL
  AND (COALESCE(OLD.read_pos, '') != COALESCE(NEW.read_pos, '') OR COALESCE(OLD.read_progress, 0) != COALESCE(NEW.read_progress, 0))
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at)
    VALUES (
        'reading_position', NEW.id,
        bokheim_mutation('reading_position', NEW.id, NEW.read_pos, COALESCE(NEW.read_progress, 0)),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at;
END;

CREATE TRIGGER trig_book_last_read
AFTER UPDATE OF last_read ON book
WHEN NEW.last_read > 0 AND (OLD.last_read IS NULL OR OLD.last_read != NEW.last_read)
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at)
    VALUES (
        'last_opened', NEW.id,
        bokheim_mutation('last_opened', NEW.id, NEW.last_read),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at;
END;

CREATE TRIGGER trig_annotation_state_insert
AFTER INSERT ON annotation
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at)
    VALUES (
        'annotation', NEW.id,
        bokheim_mutation('annotation', NEW.id, NEW.book_id, NEW.anchor_kind, NEW.anchor_cfi, NEW.anchor_page, NEW.anchor_rects, NEW.anchor_fallback_cfi,
            NEW.exact_text, NEW.prefix, NEW.suffix, NEW.style, NEW.color, NEW.note, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at;
END;

CREATE TRIGGER trig_annotation_state_update
AFTER UPDATE OF anchor_kind, anchor_cfi, anchor_page, anchor_rects, anchor_fallback_cfi, exact_text, prefix, suffix, style, color, note, modified_at, deleted_at ON annotation
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at)
    VALUES (
        'annotation', NEW.id,
        bokheim_mutation('annotation', NEW.id, NEW.book_id, NEW.anchor_kind, NEW.anchor_cfi, NEW.anchor_page, NEW.anchor_rects, NEW.anchor_fallback_cfi,
            NEW.exact_text, NEW.prefix, NEW.suffix, NEW.style, NEW.color, NEW.note, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at;
END;

CREATE TRIGGER trig_bookmark_state_insert
AFTER INSERT ON bookmark
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at)
    VALUES (
        'bookmark', NEW.id,
        bokheim_mutation('bookmark', NEW.id, NEW.book_id, NEW.cfi, NEW.label, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at;
END;

CREATE TRIGGER trig_bookmark_state_update
AFTER UPDATE OF cfi, label, modified_at, deleted_at ON bookmark
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at)
    VALUES (
        'bookmark', NEW.id,
        bokheim_mutation('bookmark', NEW.id, NEW.book_id, NEW.cfi, NEW.label, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at;
END;

CREATE TRIGGER trig_book_link_state_insert
AFTER INSERT ON book_link
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at)
    VALUES (
        'book_link', NEW.id,
        bokheim_mutation('book_link', NEW.id, NEW.source_book_id, NEW.target_book_id, NEW.relation, NEW.note, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at;
END;

CREATE TRIGGER trig_book_link_state_update
AFTER UPDATE OF source_book_id, target_book_id, relation, note, modified_at, deleted_at ON book_link
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at)
    VALUES (
        'book_link', NEW.id,
        bokheim_mutation('book_link', NEW.id, NEW.source_book_id, NEW.target_book_id, NEW.relation, NEW.note, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)
    )
    ON CONFLICT(state_kind, state_key) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at;
END;

-- Stamp local materialized state with the exact version written to the outbox.
-- These columns are not watched by producer triggers, so this cannot emit a
-- second mutation. INSERT and coalescing UPDATE use the same rules.
CREATE TRIGGER trig_sync_outbox_stamp_insert
AFTER INSERT ON sync_outbox
BEGIN
    UPDATE book SET version_changed_at=NEW.changed_at, version_tie_breaker=0,
        version_replica_id=COALESCE((SELECT value FROM sync_metadata WHERE key='client_id'), ''),
        version_replica_seq=NEW.replica_seq, version_mutation_id=NEW.mutation_id
    WHERE NEW.state_kind='book_lifecycle' AND id=NEW.state_key;
    UPDATE dir SET version_changed_at=NEW.changed_at, version_tie_breaker=0,
        version_replica_id=COALESCE((SELECT value FROM sync_metadata WHERE key='client_id'), ''),
        version_replica_seq=NEW.replica_seq, version_mutation_id=NEW.mutation_id
    WHERE NEW.state_kind='directory' AND id=NEW.state_key;
    UPDATE book_dir SET version_changed_at=NEW.changed_at, version_tie_breaker=0,
        version_replica_id=COALESCE((SELECT value FROM sync_metadata WHERE key='client_id'), ''),
        version_replica_seq=NEW.replica_seq, version_mutation_id=NEW.mutation_id
    WHERE NEW.state_kind='placement'
      AND dir_id=NEW.placement_dir_id
      AND file_name=NEW.placement_file_name
      AND book_id=NEW.placement_book_id;
    UPDATE annotation SET version_changed_at=NEW.changed_at, version_tie_breaker=0,
        version_replica_id=COALESCE((SELECT value FROM sync_metadata WHERE key='client_id'), ''),
        version_replica_seq=NEW.replica_seq, version_mutation_id=NEW.mutation_id
    WHERE NEW.state_kind='annotation' AND id=NEW.state_key;
    UPDATE bookmark SET version_changed_at=NEW.changed_at, version_tie_breaker=0,
        version_replica_id=COALESCE((SELECT value FROM sync_metadata WHERE key='client_id'), ''),
        version_replica_seq=NEW.replica_seq, version_mutation_id=NEW.mutation_id
    WHERE NEW.state_kind='bookmark' AND id=NEW.state_key;
    UPDATE book_link SET version_changed_at=NEW.changed_at, version_tie_breaker=0,
        version_replica_id=COALESCE((SELECT value FROM sync_metadata WHERE key='client_id'), ''),
        version_replica_seq=NEW.replica_seq, version_mutation_id=NEW.mutation_id
    WHERE NEW.state_kind='book_link' AND id=NEW.state_key;
END;

CREATE TRIGGER trig_sync_outbox_stamp_update
AFTER UPDATE OF replica_seq ON sync_outbox
BEGIN
    UPDATE book SET version_changed_at=NEW.changed_at, version_tie_breaker=0,
        version_replica_id=COALESCE((SELECT value FROM sync_metadata WHERE key='client_id'), ''),
        version_replica_seq=NEW.replica_seq, version_mutation_id=NEW.mutation_id
    WHERE NEW.state_kind='book_lifecycle' AND id=NEW.state_key;
    UPDATE dir SET version_changed_at=NEW.changed_at, version_tie_breaker=0,
        version_replica_id=COALESCE((SELECT value FROM sync_metadata WHERE key='client_id'), ''),
        version_replica_seq=NEW.replica_seq, version_mutation_id=NEW.mutation_id
    WHERE NEW.state_kind='directory' AND id=NEW.state_key;
    UPDATE book_dir SET version_changed_at=NEW.changed_at, version_tie_breaker=0,
        version_replica_id=COALESCE((SELECT value FROM sync_metadata WHERE key='client_id'), ''),
        version_replica_seq=NEW.replica_seq, version_mutation_id=NEW.mutation_id
    WHERE NEW.state_kind='placement'
      AND dir_id=NEW.placement_dir_id
      AND file_name=NEW.placement_file_name
      AND book_id=NEW.placement_book_id;
    UPDATE annotation SET version_changed_at=NEW.changed_at, version_tie_breaker=0,
        version_replica_id=COALESCE((SELECT value FROM sync_metadata WHERE key='client_id'), ''),
        version_replica_seq=NEW.replica_seq, version_mutation_id=NEW.mutation_id
    WHERE NEW.state_kind='annotation' AND id=NEW.state_key;
    UPDATE bookmark SET version_changed_at=NEW.changed_at, version_tie_breaker=0,
        version_replica_id=COALESCE((SELECT value FROM sync_metadata WHERE key='client_id'), ''),
        version_replica_seq=NEW.replica_seq, version_mutation_id=NEW.mutation_id
    WHERE NEW.state_kind='bookmark' AND id=NEW.state_key;
    UPDATE book_link SET version_changed_at=NEW.changed_at, version_tie_breaker=0,
        version_replica_id=COALESCE((SELECT value FROM sync_metadata WHERE key='client_id'), ''),
        version_replica_seq=NEW.replica_seq, version_mutation_id=NEW.mutation_id
    WHERE NEW.state_kind='book_link' AND id=NEW.state_key;
END;
