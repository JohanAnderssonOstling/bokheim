DROP TRIGGER IF EXISTS trig_dir_added_insert;
DROP TRIGGER IF EXISTS trig_dir_added_reactivate;
DROP TRIGGER IF EXISTS trig_dir_moved;
DROP TRIGGER IF EXISTS trig_dir_deleted;
DROP TRIGGER IF EXISTS trig_dir_state_insert;
DROP TRIGGER IF EXISTS trig_dir_state_update;
DROP TRIGGER IF EXISTS trig_dir_name_update;
DROP TRIGGER IF EXISTS trig_dir_parent_update;
DROP TRIGGER IF EXISTS trig_dir_lifecycle_update;
DROP TRIGGER IF EXISTS trig_book_added;
DROP TRIGGER IF EXISTS trig_book_added_reactivate;
DROP TRIGGER IF EXISTS trig_book_deleted;
DROP TRIGGER IF EXISTS trig_book_purged;
DROP TRIGGER IF EXISTS trig_book_dir_added_insert;
DROP TRIGGER IF EXISTS trig_book_dir_added_reactivate;
DROP TRIGGER IF EXISTS trig_book_dir_deleted;
DROP TRIGGER IF EXISTS trig_book_read_pos;
DROP TRIGGER IF EXISTS trig_book_metadata;
DROP TRIGGER IF EXISTS trig_book_contributor_insert;
DROP TRIGGER IF EXISTS trig_book_contributor_update;
DROP TRIGGER IF EXISTS trig_book_contributor_delete;
DROP TRIGGER IF EXISTS trig_annotation_state_insert;
DROP TRIGGER IF EXISTS trig_annotation_state_update;
DROP TRIGGER IF EXISTS trig_bookmark_state_insert;
DROP TRIGGER IF EXISTS trig_bookmark_state_update;
DROP TRIGGER IF EXISTS trig_book_link_state_insert;
DROP TRIGGER IF EXISTS trig_book_link_state_update;
DROP TRIGGER IF EXISTS trig_catalog_publication_insert;
DROP TRIGGER IF EXISTS trig_catalog_publication_update;
DROP TRIGGER IF EXISTS trig_sync_outbox_stamp_insert;
DROP TRIGGER IF EXISTS trig_sync_outbox_stamp_update;

-- Every producer passes typed SQL values through the registered Rust encoder.
-- Invalid protocol values abort the surrounding transaction; valid values are
-- stored as versioned bincode without an intermediate JSON representation.
CREATE TRIGGER trig_dir_state_insert
AFTER INSERT ON dir
WHEN NEW.id != '00000000-0000-0000-0000-000000000000'
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
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
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
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
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
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
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
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
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_lifecycle', NEW.content_hash,
        bokheim_mutation('book_lifecycle', NEW.content_hash, 0, COALESCE(NEW.title, NEW.content_hash), contributors_for_book(NEW.row_id), COALESCE(NEW.description, ''), NEW.publication_metadata, COALESCE(NEW.added_at, 0), NEW.format, NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_added_reactivate
AFTER UPDATE OF deleted_at, purged_at ON book
WHEN NEW.deleted_at IS NULL AND (OLD.deleted_at IS NOT NULL OR OLD.purged_at IS NOT NULL)
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_lifecycle', NEW.content_hash,
        bokheim_mutation('book_lifecycle', NEW.content_hash, 0, COALESCE(NEW.title, NEW.content_hash), contributors_for_book(NEW.row_id), COALESCE(NEW.description, ''), NEW.publication_metadata, COALESCE(NEW.added_at, 0), NEW.format, NULL),
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
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_lifecycle', NEW.content_hash,
        bokheim_mutation('book_lifecycle', NEW.content_hash, 1, '', X'', '', X'', 0, '', NEW.trash_origin_dir_id),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 1
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_purged
AFTER UPDATE OF purged_at ON book
WHEN NEW.purged_at IS NOT NULL AND OLD.purged_at IS NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_lifecycle', NEW.content_hash,
        bokheim_mutation('book_lifecycle', NEW.content_hash, 2, '', X'', '', X'', 0, '', NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 2
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_metadata
AFTER UPDATE OF title, description, publication_metadata ON book
WHEN (OLD.title IS NOT NEW.title OR OLD.description IS NOT NEW.description OR OLD.publication_metadata IS NOT NEW.publication_metadata)
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'metadata', NEW.content_hash,
        bokheim_mutation('metadata', NEW.content_hash, COALESCE(NEW.title, ''), contributors_for_book(NEW.row_id), COALESCE(NEW.description, ''), NEW.publication_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

-- book_contributor edits no longer piggyback on a `book`-row change (a
-- clear+rewrite touches zero book columns), so they get their own triggers
-- that refresh the `metadata` mutation for the affected book. The ON CONFLICT
-- coalescing (shared with trig_book_metadata above) collapses every row-level
-- firing from one contributor-list rewrite into a single outbox mutation per
-- (book_row_id, kind).
CREATE TRIGGER trig_book_contributor_insert
AFTER INSERT ON book_contributor
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    SELECT
        'metadata', book.content_hash,
        bokheim_mutation('metadata', book.content_hash, COALESCE(book.title, ''), contributors_for_book(book.row_id), COALESCE(book.description, ''), book.publication_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    FROM book WHERE book.row_id = NEW.book_row_id
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_contributor_update
AFTER UPDATE ON book_contributor
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    SELECT
        'metadata', book.content_hash,
        bokheim_mutation('metadata', book.content_hash, COALESCE(book.title, ''), contributors_for_book(book.row_id), COALESCE(book.description, ''), book.publication_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    FROM book WHERE book.row_id = NEW.book_row_id
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_contributor_delete
AFTER DELETE ON book_contributor
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
  AND EXISTS (SELECT 1 FROM book WHERE book.row_id = OLD.book_row_id)
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    SELECT
        'metadata', book.content_hash,
        bokheim_mutation('metadata', book.content_hash, COALESCE(book.title, ''), contributors_for_book(book.row_id), COALESCE(book.description, ''), book.publication_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    FROM book WHERE book.row_id = OLD.book_row_id
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_dir_added_insert
AFTER INSERT ON book_dir
WHEN NEW.deleted_at IS NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
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
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
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
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
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
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
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
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'annotation', NEW.id,
        bokheim_mutation('annotation', NEW.id, (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), NEW.anchor_kind, NEW.anchor_cfi, NEW.anchor_page, NEW.anchor_rects, NEW.anchor_fallback_cfi,
            NEW.exact_text, NEW.style, NEW.color, NEW.note, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.deleted_at IS NOT NULL
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_annotation_state_update
AFTER UPDATE OF anchor_kind, anchor_cfi, anchor_page, anchor_rects, anchor_fallback_cfi, exact_text, style, color, note, modified_at, deleted_at ON annotation
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'annotation', NEW.id,
        bokheim_mutation('annotation', NEW.id, (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), NEW.anchor_kind, NEW.anchor_cfi, NEW.anchor_page, NEW.anchor_rects, NEW.anchor_fallback_cfi,
            NEW.exact_text, NEW.style, NEW.color, NEW.note, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.deleted_at IS NOT NULL
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_bookmark_state_insert
AFTER INSERT ON bookmark
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'bookmark', NEW.id,
        bokheim_mutation('bookmark', NEW.id, (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), NEW.cfi, NEW.label, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.deleted_at IS NOT NULL
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_bookmark_state_update
AFTER UPDATE OF deleted_at ON bookmark
WHEN OLD.deleted_at IS NULL AND NEW.deleted_at IS NOT NULL
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'bookmark', NEW.id,
        bokheim_mutation('bookmark', NEW.id, (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id), NEW.cfi, NEW.label, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.deleted_at IS NOT NULL
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_link_state_insert
AFTER INSERT ON book_link
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_link', NEW.id,
        bokheim_mutation('book_link', NEW.id,
            (SELECT content_hash FROM book WHERE row_id=NEW.source_book_row_id),
            (SELECT content_hash FROM book WHERE row_id=NEW.target_book_row_id),
            NEW.relation, NEW.note, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.deleted_at IS NOT NULL
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_book_link_state_update
AFTER UPDATE OF source_book_row_id, target_book_row_id, relation, note, modified_at, deleted_at ON book_link
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'book_link', NEW.id,
        bokheim_mutation('book_link', NEW.id,
            (SELECT content_hash FROM book WHERE row_id=NEW.source_book_row_id),
            (SELECT content_hash FROM book WHERE row_id=NEW.target_book_row_id),
            NEW.relation, NEW.note, NEW.created_at, NEW.modified_at, NEW.deleted_at IS NOT NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), NEW.deleted_at IS NOT NULL
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_catalog_publication_insert
AFTER INSERT ON catalog_publication_book
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, state_subkey, body, changed_at, conflict_rank)
    VALUES (
        'catalog_publication', NEW.source_id,
        catalog_publication_state_subkey(NEW.edition_id, NULLIF(NEW.work_id, '')),
        bokheim_mutation('catalog_publication', NEW.source_id, NEW.edition_id,
            NULLIF(NEW.work_id, ''),
            (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id)),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER trig_catalog_publication_update
AFTER UPDATE OF book_row_id ON catalog_publication_book
WHEN OLD.book_row_id IS NOT NEW.book_row_id
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, state_subkey, body, changed_at, conflict_rank)
    VALUES (
        'catalog_publication', NEW.source_id,
        catalog_publication_state_subkey(NEW.edition_id, NULLIF(NEW.work_id, '')),
        bokheim_mutation('catalog_publication', NEW.source_id, NEW.edition_id,
            NULLIF(NEW.work_id, ''),
            (SELECT content_hash FROM book WHERE row_id=NEW.book_row_id)),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
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
        COALESCE((SELECT value FROM sync_metadata WHERE key='replica_id'), ''),NEW.replica_seq,NEW.mutation_id)
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,
        conflict_rank=excluded.conflict_rank,replica_id=excluded.replica_id,
        replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id;
END;

CREATE TRIGGER trig_sync_outbox_stamp_update
AFTER UPDATE OF replica_seq ON sync_outbox
BEGIN
    INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id)
    VALUES (NEW.state_kind, NEW.state_key, NEW.state_subkey, NEW.changed_at, NEW.conflict_rank,
        COALESCE((SELECT value FROM sync_metadata WHERE key='replica_id'), ''),NEW.replica_seq,NEW.mutation_id)
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,
        conflict_rank=excluded.conflict_rank,replica_id=excluded.replica_id,
        replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id;
END;
