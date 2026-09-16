-- Triggers that stamp local writes into the synchronization outbox.
CREATE TRIGGER trig_book_browse_format_insert
AFTER INSERT ON book
BEGIN
    INSERT INTO book_browse_format(book_row_id,format_category)
    VALUES(NEW.row_id,CASE lower(NEW.format) WHEN 'pdf' THEN 2 WHEN 'm4b' THEN 4 ELSE 1 END)
    ON CONFLICT(book_row_id) DO UPDATE SET format_category=excluded.format_category;
END;

CREATE TRIGGER trig_book_browse_format_update
AFTER UPDATE OF format ON book
BEGIN
    INSERT INTO book_browse_format(book_row_id,format_category)
    VALUES(NEW.row_id,CASE lower(NEW.format) WHEN 'pdf' THEN 2 WHEN 'm4b' THEN 4 ELSE 1 END)
    ON CONFLICT(book_row_id) DO UPDATE SET format_category=excluded.format_category;
END;

CREATE TRIGGER trig_book_browse_language_insert
AFTER INSERT ON book_language
BEGIN
    DELETE FROM book_browse_language WHERE book_row_id=NEW.book_row_id;
    INSERT INTO book_browse_language(book_row_id,base_language)
    SELECT DISTINCT book_row_id,
        CASE WHEN instr(replace(lower(trim(language_tag)),'_','-'),'-')>0
            THEN substr(replace(lower(trim(language_tag)),'_','-'),1,instr(replace(lower(trim(language_tag)),'_','-'),'-')-1)
            ELSE replace(lower(trim(language_tag)),'_','-') END
    FROM book_language WHERE book_row_id=NEW.book_row_id AND trim(language_tag)!='';
END;

CREATE TRIGGER trig_book_browse_language_update
AFTER UPDATE OF language_tag,book_row_id ON book_language
BEGIN
    DELETE FROM book_browse_language WHERE book_row_id IN (OLD.book_row_id,NEW.book_row_id);
    INSERT OR IGNORE INTO book_browse_language(book_row_id,base_language)
    SELECT DISTINCT book_row_id,
        CASE WHEN instr(replace(lower(trim(language_tag)),'_','-'),'-')>0
            THEN substr(replace(lower(trim(language_tag)),'_','-'),1,instr(replace(lower(trim(language_tag)),'_','-'),'-')-1)
            ELSE replace(lower(trim(language_tag)),'_','-') END
    FROM book_language WHERE book_row_id IN (OLD.book_row_id,NEW.book_row_id) AND trim(language_tag)!='';
END;

CREATE TRIGGER trig_book_browse_language_delete
AFTER DELETE ON book_language
BEGIN
    DELETE FROM book_browse_language WHERE book_row_id=OLD.book_row_id;
    INSERT INTO book_browse_language(book_row_id,base_language)
    SELECT DISTINCT book_row_id,
        CASE WHEN instr(replace(lower(trim(language_tag)),'_','-'),'-')>0
            THEN substr(replace(lower(trim(language_tag)),'_','-'),1,instr(replace(lower(trim(language_tag)),'_','-'),'-')-1)
            ELSE replace(lower(trim(language_tag)),'_','-') END
    FROM book_language WHERE book_row_id=OLD.book_row_id AND trim(language_tag)!='';
END;

-- Assignment writes only invalidate counts. Browsing rebuilds them once per
-- batch, using the final shared taxonomy, instead of walking all routes for
-- every single assignment inserted during an import.
CREATE TRIGGER trig_subject_book_ownership_insert
AFTER INSERT ON book_unified_concept
BEGIN UPDATE browse_projection_state SET dirty=1 WHERE projection='subject_ownership'; END;
CREATE TRIGGER trig_subject_book_ownership_delete
AFTER DELETE ON book_unified_concept
BEGIN UPDATE browse_projection_state SET dirty=1 WHERE projection='subject_ownership'; END;

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
        bokheim_mutation('book_lifecycle', NEW.content_hash, 0, COALESCE(NEW.title, NEW.content_hash), contributors_for_book(NEW.row_id), NEW.publication_metadata, COALESCE(NEW.added_at, 0), NEW.format, NULL),
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
        bokheim_mutation('book_lifecycle', NEW.content_hash, 0, COALESCE(NEW.title, NEW.content_hash), contributors_for_book(NEW.row_id), NEW.publication_metadata, COALESCE(NEW.added_at, 0), NEW.format, NULL),
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
        bokheim_mutation('book_lifecycle', NEW.content_hash, 1, '', X'', X'', 0, '', NEW.trash_origin_dir_id),
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
        bokheim_mutation('book_lifecycle', NEW.content_hash, 2, '', X'', X'', 0, '', NULL),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 2
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;


-- Folder ownership is derived state, not synchronized state. These triggers
-- deliberately run for both local and remote placement writes.
CREATE TRIGGER trig_folder_book_ownership_insert
AFTER INSERT ON book_dir
WHEN NEW.deleted_at IS NULL
BEGIN
    INSERT INTO folder_book_ownership(folder_id, book_row_id, placement_count, downloaded_placement_count)
    WITH RECURSIVE ancestors(id) AS (
        SELECT id FROM dir WHERE id=NEW.dir_id AND deleted_at IS NULL
        UNION ALL
        SELECT parent.id
        FROM ancestors AS child
        JOIN dir AS current ON current.id=child.id
        JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
        WHERE current.id != current.parent_id
    )
    SELECT id, NEW.book_row_id, 1, CASE WHEN NEW.is_downloaded=1 THEN 1 ELSE 0 END
    FROM ancestors
    WHERE true
    ON CONFLICT(folder_id, book_row_id) DO UPDATE SET
        placement_count=folder_book_ownership.placement_count + 1,
        downloaded_placement_count=folder_book_ownership.downloaded_placement_count + excluded.downloaded_placement_count;
END;

-- Remove OLD's contribution before any tracked placement attribute changes.
-- Rows at one are deleted first so the positive-count constraint is never
-- transiently violated.
CREATE TRIGGER trig_folder_book_ownership_update_remove
BEFORE UPDATE OF dir_id, book_row_id, deleted_at, is_downloaded ON book_dir
WHEN OLD.deleted_at IS NULL
BEGIN
    DELETE FROM folder_book_ownership
    WHERE book_row_id=OLD.book_row_id
      AND placement_count=1
      AND folder_id IN (
          WITH RECURSIVE ancestors(id) AS (
              SELECT id FROM dir WHERE id=OLD.dir_id AND deleted_at IS NULL
              UNION ALL
              SELECT parent.id
              FROM ancestors AS child
              JOIN dir AS current ON current.id=child.id
              JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
              WHERE current.id != current.parent_id
          )
          SELECT id FROM ancestors
      );
    UPDATE folder_book_ownership
    SET placement_count=placement_count - 1,
        downloaded_placement_count=downloaded_placement_count - CASE WHEN OLD.is_downloaded=1 THEN 1 ELSE 0 END
    WHERE book_row_id=OLD.book_row_id
      AND folder_id IN (
          WITH RECURSIVE ancestors(id) AS (
              SELECT id FROM dir WHERE id=OLD.dir_id AND deleted_at IS NULL
              UNION ALL
              SELECT parent.id
              FROM ancestors AS child
              JOIN dir AS current ON current.id=child.id
              JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
              WHERE current.id != current.parent_id
          )
          SELECT id FROM ancestors
      );
END;

CREATE TRIGGER trig_folder_book_ownership_update_add
AFTER UPDATE OF dir_id, book_row_id, deleted_at, is_downloaded ON book_dir
WHEN NEW.deleted_at IS NULL
BEGIN
    INSERT INTO folder_book_ownership(folder_id, book_row_id, placement_count, downloaded_placement_count)
    WITH RECURSIVE ancestors(id) AS (
        SELECT id FROM dir WHERE id=NEW.dir_id AND deleted_at IS NULL
        UNION ALL
        SELECT parent.id
        FROM ancestors AS child
        JOIN dir AS current ON current.id=child.id
        JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
        WHERE current.id != current.parent_id
    )
    SELECT id, NEW.book_row_id, 1, CASE WHEN NEW.is_downloaded=1 THEN 1 ELSE 0 END
    FROM ancestors
    WHERE true
    ON CONFLICT(folder_id, book_row_id) DO UPDATE SET
        placement_count=folder_book_ownership.placement_count + 1,
        downloaded_placement_count=folder_book_ownership.downloaded_placement_count + excluded.downloaded_placement_count;
END;

CREATE TRIGGER trig_folder_book_ownership_delete
BEFORE DELETE ON book_dir
WHEN OLD.deleted_at IS NULL
BEGIN
    DELETE FROM folder_book_ownership
    WHERE book_row_id=OLD.book_row_id
      AND placement_count=1
      AND folder_id IN (
          WITH RECURSIVE ancestors(id) AS (
              SELECT id FROM dir WHERE id=OLD.dir_id AND deleted_at IS NULL
              UNION ALL
              SELECT parent.id
              FROM ancestors AS child
              JOIN dir AS current ON current.id=child.id
              JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
              WHERE current.id != current.parent_id
          )
          SELECT id FROM ancestors
      );
    UPDATE folder_book_ownership
    SET placement_count=placement_count - 1,
        downloaded_placement_count=downloaded_placement_count - CASE WHEN OLD.is_downloaded=1 THEN 1 ELSE 0 END
    WHERE book_row_id=OLD.book_row_id
      AND folder_id IN (
          WITH RECURSIVE ancestors(id) AS (
              SELECT id FROM dir WHERE id=OLD.dir_id AND deleted_at IS NULL
              UNION ALL
              SELECT parent.id
              FROM ancestors AS child
              JOIN dir AS current ON current.id=child.id
              JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
              WHERE current.id != current.parent_id
          )
          SELECT id FROM ancestors
      );
END;

-- Folder hierarchy changes are much rarer than placement writes. Rebuilding
-- once gives exact ownership across the moved subtree and both ancestor
-- chains. Remote projection suppresses this per-row rebuild and performs one
-- rebuild after its batch instead.
CREATE TRIGGER trig_folder_book_ownership_directory_update
AFTER UPDATE OF parent_id, deleted_at, purged_at ON dir
WHEN (OLD.parent_id IS NOT NEW.parent_id OR OLD.deleted_at IS NOT NEW.deleted_at OR OLD.purged_at IS NOT NEW.purged_at)
  AND COALESCE((SELECT value FROM sync_metadata WHERE key='change_origin'), 'local') != 'remote'
BEGIN
    DELETE FROM folder_book_ownership;
    INSERT INTO folder_book_ownership(folder_id, book_row_id, placement_count, downloaded_placement_count)
    WITH RECURSIVE ancestry(descendant_id, ancestor_id) AS (
        SELECT id, id FROM dir WHERE deleted_at IS NULL
        UNION ALL
        SELECT ancestry.descendant_id, parent.id
        FROM ancestry
        JOIN dir AS current ON current.id=ancestry.ancestor_id
        JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
        WHERE current.id != current.parent_id
    )
    SELECT ancestry.ancestor_id, placement.book_row_id, COUNT(*),
           SUM(CASE WHEN placement.is_downloaded=1 THEN 1 ELSE 0 END)
    FROM book_dir AS placement
    JOIN ancestry ON ancestry.descendant_id=placement.dir_id
    WHERE placement.deleted_at IS NULL
    GROUP BY ancestry.ancestor_id, placement.book_row_id;
END;

CREATE TRIGGER trig_folder_book_ownership_directory_delete
AFTER DELETE ON dir
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key='change_origin'), 'local') != 'remote'
BEGIN
    DELETE FROM folder_book_ownership;
    INSERT INTO folder_book_ownership(folder_id, book_row_id, placement_count, downloaded_placement_count)
    WITH RECURSIVE ancestry(descendant_id, ancestor_id) AS (
        SELECT id, id FROM dir WHERE deleted_at IS NULL
        UNION ALL
        SELECT ancestry.descendant_id, parent.id
        FROM ancestry
        JOIN dir AS current ON current.id=ancestry.ancestor_id
        JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
        WHERE current.id != current.parent_id
    )
    SELECT ancestry.ancestor_id, placement.book_row_id, COUNT(*),
           SUM(CASE WHEN placement.is_downloaded=1 THEN 1 ELSE 0 END)
    FROM book_dir AS placement
    JOIN ancestry ON ancestry.descendant_id=placement.dir_id
    WHERE placement.deleted_at IS NULL
    GROUP BY ancestry.ancestor_id, placement.book_row_id;
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

