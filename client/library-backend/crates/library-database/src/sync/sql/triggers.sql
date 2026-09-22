-- name: schema_sync_triggers &

-- Every producer passes typed SQL values through the registered Rust encoder.
-- Invalid protocol values abort the surrounding transaction; valid values are
-- stored as versioned bincode without an intermediate JSON representation.
CREATE TRIGGER trig_dir_state_insert
AFTER INSERT ON dir
WHEN NEW.id != '00000000-0000-0000-0000-000000000000'
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'directory_name', NEW.id,
        bokheim_mutation('directory_name', NEW.id, NEW.name),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'directory_parent', NEW.id,
        bokheim_mutation('directory_parent', NEW.id, NEW.parent_id),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'directory_lifecycle', NEW.id,
        bokheim_mutation('directory_lifecycle', NEW.id, CASE WHEN NEW.purged_at IS NOT NULL THEN 2 WHEN NEW.deleted_at IS NOT NULL THEN 1 ELSE 0 END),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), CASE WHEN NEW.purged_at IS NOT NULL THEN 2 WHEN NEW.deleted_at IS NOT NULL THEN 1 ELSE 0 END
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_dir_name_update
AFTER UPDATE OF name ON dir
WHEN NEW.id != '00000000-0000-0000-0000-000000000000'
  AND OLD.name IS NOT NEW.name
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'directory_name', NEW.id,
        bokheim_mutation('directory_name', NEW.id, NEW.name),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_dir_parent_update
AFTER UPDATE OF parent_id ON dir
WHEN NEW.id != '00000000-0000-0000-0000-000000000000' AND OLD.parent_id IS NOT NEW.parent_id
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES ('directory_parent', NEW.id, bokheim_mutation('directory_parent', NEW.id, NEW.parent_id),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0)
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET replica_seq=excluded.replica_seq,
        mutation_id=excluded.mutation_id, body=excluded.body, changed_at=excluded.changed_at,
        conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_dir_lifecycle_update
AFTER UPDATE OF deleted_at, purged_at ON dir
WHEN NEW.id != '00000000-0000-0000-0000-000000000000'
  AND (OLD.deleted_at IS NOT NEW.deleted_at OR OLD.purged_at IS NOT NEW.purged_at)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES ('directory_lifecycle', NEW.id, bokheim_mutation('directory_lifecycle', NEW.id, CASE WHEN NEW.purged_at IS NOT NULL THEN 2 WHEN NEW.deleted_at IS NOT NULL THEN 1 ELSE 0 END),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), CASE WHEN NEW.purged_at IS NOT NULL THEN 2 WHEN NEW.deleted_at IS NOT NULL THEN 1 ELSE 0 END)
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET replica_seq=excluded.replica_seq,
        mutation_id=excluded.mutation_id, body=excluded.body, changed_at=excluded.changed_at,
        conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_added
AFTER INSERT ON book
WHEN NEW.deleted_at IS NULL
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_lifecycle', NEW.content_hash,
        bokheim_mutation('book_lifecycle', NEW.content_hash, 0, COALESCE(NEW.title, NEW.content_hash), NEW.subtitle, contributors_for_book(NEW.row_id), NEW.book_metadata, COALESCE(NEW.added_at, 0), NEW.format, NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_added_reactivate
AFTER UPDATE OF deleted_at, purged_at ON book
WHEN NEW.deleted_at IS NULL AND (OLD.deleted_at IS NOT NULL OR OLD.purged_at IS NOT NULL)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_lifecycle', NEW.content_hash,
        bokheim_mutation('book_lifecycle', NEW.content_hash, 0, COALESCE(NEW.title, NEW.content_hash), NEW.subtitle, contributors_for_book(NEW.row_id), NEW.book_metadata, COALESCE(NEW.added_at, 0), NEW.format, NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_deleted
AFTER UPDATE OF deleted_at, purged_at ON book
WHEN NEW.deleted_at IS NOT NULL AND NEW.purged_at IS NULL
  AND (OLD.deleted_at IS NULL OR OLD.purged_at IS NOT NULL)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_lifecycle', NEW.content_hash,
        bokheim_mutation('book_lifecycle', NEW.content_hash, 1, '', NULL, X'', X'', 0, '', NEW.trash_origin_dir_id),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 1
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_purged
AFTER UPDATE OF purged_at ON book
WHEN NEW.purged_at IS NOT NULL AND OLD.purged_at IS NULL
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_lifecycle', NEW.content_hash,
        bokheim_mutation('book_lifecycle', NEW.content_hash, 2, '', NULL, X'', X'', 0, '', NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 2
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_dir_added_insert
AFTER INSERT ON book_dir
WHEN NEW.deleted_at IS NULL
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, state_subkey, body, changed_at, conflict_rank)
    VALUES (
        'placement', (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), NEW.dir_id,
        bokheim_mutation('placement', NEW.dir_id, (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), 1, NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    ) ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_dir_added_reactivate
AFTER UPDATE OF deleted_at, dir_id, book_row_id ON book_dir
WHEN NEW.deleted_at IS NULL
  AND (OLD.deleted_at IS NOT NULL OR OLD.dir_id IS NOT NEW.dir_id OR OLD.book_row_id IS NOT NEW.book_row_id)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, state_subkey, body, changed_at, conflict_rank)
    VALUES (
        'placement', (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), NEW.dir_id,
        bokheim_mutation('placement', NEW.dir_id, (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), 1, NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    ) ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_dir_deleted
AFTER UPDATE OF deleted_at ON book_dir
WHEN NEW.deleted_at IS NOT NULL AND OLD.deleted_at IS NULL
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, state_subkey, body, changed_at, conflict_rank)
    VALUES (
        'placement', (SELECT content_hash FROM book WHERE row_id=OLD.book_row_id), OLD.dir_id,
        bokheim_mutation('placement', OLD.dir_id, (SELECT content_hash FROM book WHERE row_id=OLD.book_row_id), 0, NEW.trash_origin_dir_id),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 1
    ) ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_read_pos
AFTER UPDATE OF read_pos, read_progress ON book
WHEN NEW.read_pos IS NOT NULL
  AND (COALESCE(OLD.read_pos, '') != COALESCE(NEW.read_pos, '') OR COALESCE(OLD.read_progress, 0) != COALESCE(NEW.read_progress, 0))
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'reading_position', NEW.content_hash,
        bokheim_mutation('reading_position', NEW.content_hash, NEW.read_pos, COALESCE(NEW.read_progress, 0)),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_annotation_state_insert
AFTER INSERT ON annotation
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'annotation', NEW.id,
        bokheim_mutation('annotation', NEW.id, (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), NEW.detail, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.deleted_at IS NOT NULL
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_annotation_state_update
AFTER UPDATE OF detail, modified_at, deleted_at ON annotation
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'annotation', NEW.id,
        bokheim_mutation('annotation', NEW.id, (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), NEW.detail, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.deleted_at IS NOT NULL
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_sync_outbox_stamp_insert
AFTER INSERT ON sync_outbox
BEGIN
    INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id)
    VALUES (NEW.state_kind,
        NEW.state_key, NEW.state_subkey, NEW.changed_at, NEW.conflict_rank,
        COALESCE((SELECT replica_id FROM sync_metadata WHERE singleton = 1), ''),NEW.replica_seq,NEW.mutation_id)
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,
        conflict_rank=excluded.conflict_rank,replica_id=excluded.replica_id,
        replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id;
END;

CREATE TRIGGER trig_sync_outbox_stamp_update
AFTER UPDATE OF replica_seq ON sync_outbox
BEGIN
    INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id)
    VALUES (NEW.state_kind, NEW.state_key, NEW.state_subkey, NEW.changed_at, NEW.conflict_rank,
        COALESCE((SELECT replica_id FROM sync_metadata WHERE singleton = 1), ''),NEW.replica_seq,NEW.mutation_id)
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,
        conflict_rank=excluded.conflict_rank,replica_id=excluded.replica_id,
        replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id;
END;
-- Navigation travels because a replica cannot always rebuild it: an audiobook's
-- chapters may have come from enrichment rather than from the file, and a
-- replica that has not downloaded a book has no file to rebuild from at all.
CREATE TRIGGER IF NOT EXISTS book_toc_insert AFTER INSERT ON book_toc
BEGIN
    INSERT INTO sync_outbox(state_kind,state_key,state_subkey,body,changed_at,conflict_rank)
    SELECT 'book_toc',NEW.content_hash,'',bokheim_mutation('book_toc',NEW.content_hash,NEW.toc_json,COALESCE((SELECT duration_ms FROM audiobook_metadata WHERE content_hash=NEW.content_hash),-1)),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    WHERE COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER IF NOT EXISTS book_toc_update AFTER UPDATE ON book_toc
BEGIN
    INSERT INTO sync_outbox(state_kind,state_key,state_subkey,body,changed_at,conflict_rank)
    SELECT 'book_toc',NEW.content_hash,'',bokheim_mutation('book_toc',NEW.content_hash,NEW.toc_json,COALESCE((SELECT duration_ms FROM audiobook_metadata WHERE content_hash=NEW.content_hash),-1)),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    WHERE COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER IF NOT EXISTS book_toc_purge AFTER UPDATE OF purged_at ON book
WHEN NEW.purged_at IS NOT NULL
BEGIN
    DELETE FROM book_toc WHERE content_hash=NEW.content_hash;
    DELETE FROM sync_outbox WHERE state_kind='book_toc' AND state_key=NEW.content_hash;
END;

