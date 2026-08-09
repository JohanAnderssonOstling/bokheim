-- name: create_views &
CREATE VIEW IF NOT EXISTS dir_paths AS
WITH RECURSIVE dir_path(id, path) AS (
    -- root produces empty path
    SELECT id, ''
    FROM dir
    WHERE parent_id = id
      AND deleted_at IS NULL

    UNION ALL

    -- children append normally
    SELECT
        d.id,
        CASE
            WHEN dp.path = ''
                THEN d.name
            ELSE dp.path || '/' || d.name
        END
    FROM dir d
    JOIN dir_path dp ON d.parent_id = dp.id
    WHERE d.deleted_at IS NULL
      AND d.id <> d.parent_id
)
SELECT id, path FROM dir_path;

-- Author-role subset of the merged contributor register. Kept as a view so
-- the `role = X'617574'` (MarcRelatorCode::AUTHOR) literal is not repeated
-- across every query that only cares about credited authors.
CREATE VIEW IF NOT EXISTS author_credit AS
SELECT book_id, position, author_identity_id, name AS credited_name, sort_name
FROM book_contributor
WHERE role = X'617574';

CREATE VIEW IF NOT EXISTS book_cards AS
SELECT
    book.id AS book_id,
    book.title,
    COALESCE((
        SELECT group_concat(credit.credited_name, ', ')
        FROM (SELECT credited_name FROM author_credit WHERE book_id = book.id ORDER BY position) AS credit
    ), '') AS author,
    book.description,
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
        WHERE placement.book_id = book.id
          AND placement.deleted_at IS NULL
          AND placement.is_downloaded = 1
    ) AS downloaded,
    EXISTS (
        SELECT 1 FROM book_dir placement
        JOIN download_requests request
          ON request.target_kind = 'book'
         AND request.target_key = book.content_hash
        WHERE placement.book_id = book.id AND placement.deleted_at IS NULL
    ) AS download_requested,
    EXISTS (
        SELECT 1 FROM book_dir placement
        WHERE placement.book_id = book.id AND placement.deleted_at IS NULL
          AND LOWER(placement.file_name) LIKE '%.m4b'
    ) AS audiobook,
    (
        SELECT metadata.duration_ms FROM book_dir placement
        JOIN audiobook_metadata metadata ON metadata.content_hash = book.content_hash
        WHERE placement.book_id = book.id AND placement.deleted_at IS NULL
        ORDER BY placement.rowid DESC LIMIT 1
    ) AS audiobook_duration_ms,
    (
        SELECT metadata.chapter_count FROM book_dir placement
        JOIN audiobook_metadata metadata ON metadata.content_hash = book.content_hash
        WHERE placement.book_id = book.id AND placement.deleted_at IS NULL
        ORDER BY placement.rowid DESC LIMIT 1
    ) AS audiobook_chapter_count
FROM book
WHERE book.deleted_at IS NULL
  AND book.hidden_at IS NULL
  AND EXISTS (
      SELECT 1 FROM book_dir placement
      WHERE placement.book_id = book.id AND placement.deleted_at IS NULL
  );
