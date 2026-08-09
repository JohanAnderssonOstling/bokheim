-- Home-page book lists and per-book reader/access state.

-- name: get_recently_read_cards?
-- param: limit: i32
-- param: downloaded_only: i32
SELECT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.audiobook,
       card.audiobook_duration_ms, card.audiobook_chapter_count
FROM book_cards card
JOIN sync_state_version reading_version
  ON reading_version.state_kind = 'reading_position'
 AND reading_version.state_key = card.content_hash
WHERE (:downloaded_only = 0 OR card.downloaded)
ORDER BY reading_version.changed_at DESC
LIMIT :limit;

-- name: get_recently_added_cards?
-- param: limit: i32
-- param: downloaded_only: i32
SELECT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.audiobook,
       card.audiobook_duration_ms, card.audiobook_chapter_count
FROM book_cards card
JOIN book ON book.row_id = card.book_row_id
WHERE (:downloaded_only = 0 OR card.downloaded)
ORDER BY COALESCE(book.added_at, 0) DESC, LOWER(COALESCE(card.title, '')), card.content_hash
LIMIT :limit;

-- name: count_all_books?
-- Counts only books with a live placement, matching what the carousels display.
SELECT COUNT(*) FROM book
WHERE deleted_at IS NULL
  AND hidden_at IS NULL
  AND EXISTS (SELECT 1 FROM book_dir bd WHERE bd.book_row_id = book.row_id AND bd.deleted_at IS NULL);

-- name: count_books_with_added_at?
SELECT COUNT(*) FROM book
WHERE deleted_at IS NULL AND added_at IS NOT NULL AND added_at > 0
  AND hidden_at IS NULL
  AND EXISTS (SELECT 1 FROM book_dir bd WHERE bd.book_row_id = book.row_id AND bd.deleted_at IS NULL);

-- name: get_book_file_path?
-- param: content_hash: &str
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '__root__' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name,
        CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END
    FROM dir d
    JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id
    WHERE d.deleted_at IS NULL
)
SELECT CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
WHERE bd.book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash) AND bd.deleted_at IS NULL
LIMIT 1;

-- name: get_book_file_path_components?
-- param: content_hash: &str
WITH RECURSIVE path_chain(id, parent_id, name, level) AS (
    -- Start from the directory containing the book
    SELECT d.id, d.parent_id, d.name, 0 AS level
    FROM dir d
    JOIN book_dir bd ON bd.dir_id = d.id
    WHERE bd.book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash) AND bd.deleted_at IS NULL
    UNION ALL
    -- Walk up to parent directories
    SELECT d.id, d.parent_id, d.name, pc.level + 1
    FROM dir d
    JOIN path_chain pc ON pc.parent_id = d.id
    WHERE d.deleted_at IS NULL AND d.id != '__root__'
)
SELECT path_component
FROM (
    -- Get all directory components in order (root to leaf), excluding __root__
    SELECT name AS path_component, level
    FROM path_chain
    WHERE id != '__root__'
    UNION ALL
    -- Add the filename as the final component
    SELECT file_name AS path_component, -1 AS level
    FROM book_dir
    WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash) AND deleted_at IS NULL
)
ORDER BY level DESC;

-- name: get_book_reading_position?
-- param: content_hash: &str
SELECT read_pos FROM book WHERE content_hash = :content_hash AND deleted_at IS NULL;

-- name: is_book_downloaded?
-- param: content_hash: &str
SELECT COUNT(*) > 0
FROM book_dir
WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
  AND deleted_at is null
  AND is_downloaded = 1;

-- name: is_book_download_requested?
-- param: content_hash: &str
SELECT
    EXISTS (
        SELECT 1
        FROM book_dir placement
        JOIN book ON book.row_id = placement.book_row_id
        JOIN download_requests request
          ON request.target_kind = 'book'
         AND request.target_key = book.content_hash
        WHERE book.content_hash = :content_hash
          AND placement.deleted_at IS NULL
    );

-- name: update_reading_position!
-- param: content_hash: &str
-- param: cfi: &str
-- param: progress: f32
UPDATE book SET read_pos = :cfi, read_progress = :progress WHERE content_hash = :content_hash;

-- name: update_pdf_reading_position!
-- param: content_hash: &str
-- param: cfi: &str
UPDATE book SET read_pos = :cfi WHERE content_hash = :content_hash;
