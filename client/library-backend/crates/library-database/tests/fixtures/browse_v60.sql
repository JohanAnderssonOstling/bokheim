-- Restore version 60 derived structures around unchanged library records.
DROP VIEW book_cards;
DROP TRIGGER browse_text_book_insert;
DROP TRIGGER browse_text_book_update;
DROP TRIGGER browse_text_credit_insert;
DROP TRIGGER browse_text_credit_delete;
DROP TRIGGER browse_text_credit_update;
DROP TABLE book_browse_text;
DROP VIEW book_browse_text_source;
DROP TRIGGER browse_progress_insert;
DROP TRIGGER browse_progress_update;
DROP TABLE book_browse_progress;
DROP TRIGGER folder_dirty_placement_insert;
DROP TRIGGER folder_dirty_placement_update;
DROP TRIGGER folder_dirty_placement_delete;
DROP TRIGGER IF EXISTS folder_dirty_directory_insert;
DROP TRIGGER folder_dirty_directory_update;
DROP TRIGGER folder_dirty_directory_delete;
DROP TABLE folder_book_ownership;
DROP TABLE folder_browse_dirty_book;
DELETE FROM browse_projection_state WHERE projection='folder_ownership';
CREATE TABLE IF NOT EXISTS folder_book_ownership(
    folder_id                    TEXT NOT NULL REFERENCES dir(id) ON DELETE CASCADE,
    book_row_id                  INTEGER NOT NULL REFERENCES book(row_id) ON DELETE CASCADE,
    placement_count              INTEGER NOT NULL CHECK(placement_count > 0),
    downloaded_placement_count   INTEGER NOT NULL CHECK(downloaded_placement_count >= 0 AND downloaded_placement_count <= placement_count),
    PRIMARY KEY(folder_id, book_row_id)
);

-- Foreign-key cascades and book-scoped diagnostics need the reverse lookup.
CREATE INDEX IF NOT EXISTS idx_folder_book_ownership_book
ON folder_book_ownership(book_row_id, folder_id);

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
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') != 'remote'
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
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') != 'remote'
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

CREATE VIEW IF NOT EXISTS book_cards AS
SELECT
    book.row_id AS book_row_id,
    book.content_hash,
    book.title,
    COALESCE(book.added_at, 0) AS added_at,
    book.subtitle AS subtitle,
    COALESCE((
        SELECT group_concat(credit.credited_name, ', ')
        FROM (SELECT credited_name FROM author_credit WHERE book_row_id = book.row_id ORDER BY position) AS credit
    ), '') AS author,
    COALESCE((SELECT description FROM book_description WHERE book_row_id=book.row_id), '') AS description,
    CASE
        WHEN book.read_progress > 0 THEN book.read_progress
        WHEN book.read_pos IS NOT NULL
            AND book.read_pos NOT IN ('', '0')
            AND CAST(book.read_pos AS REAL) > 0
            AND CAST(book.read_pos AS REAL) <= 1
            THEN CAST(book.read_pos AS REAL) * 100
        WHEN book.read_pos IS NOT NULL
            AND book.read_pos NOT IN ('', '0')
            AND CAST(book.read_pos AS REAL) > 1
            AND CAST(book.read_pos AS REAL) <= 100
            THEN CAST(book.read_pos AS REAL)
        ELSE 0
    END AS progress,
    EXISTS (
        SELECT 1 FROM book_dir placement
        WHERE placement.book_row_id = book.row_id
          AND placement.deleted_at IS NULL
          AND placement.is_downloaded = 1
    ) AS downloaded,
    EXISTS (
        SELECT 1 FROM book_dir placement
        JOIN download_requests request
          ON request.target_kind = 'book'
         AND request.target_key = book.content_hash
        WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
    ) AS download_requested,
    CASE lower(book.format)
        WHEN 'pdf' THEN 2
        WHEN 'm4b' THEN 4
        ELSE 1
    END AS format_category,
    (
        SELECT metadata.duration_ms FROM book_dir placement
        JOIN audiobook_metadata metadata ON metadata.content_hash = book.content_hash
        WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
        ORDER BY placement.rowid DESC LIMIT 1
    ) AS audiobook_duration_ms,
    (
        SELECT toc.entry_count FROM book_dir placement
        JOIN book_toc toc ON toc.content_hash = book.content_hash
        JOIN audiobook_metadata metadata ON metadata.content_hash = book.content_hash
        WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
        ORDER BY placement.rowid DESC LIMIT 1
    ) AS audiobook_chapter_count
FROM book
WHERE book.deleted_at IS NULL
  AND book.hidden_at IS NULL
  AND EXISTS (
      SELECT 1 FROM book_dir placement
      WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
  );

PRAGMA user_version=60;
