-- name: recently_read_books?
-- param: limit: i32
-- param: downloaded_only: i32
-- param: finished_threshold: f64
-- The limit applies to books in progress and to finished books separately.
-- A single LIMIT over the whole list let a run of finished books crowd out
-- the ones still being read, which is the list Home exists to show; ranking
-- within each group spends the limit twice in one statement rather than
-- asking for the two lists in two round trips.
WITH read_books AS (
  SELECT book.row_id, book.search_title, book.title, book.content_hash, book.read_at,
         COALESCE(book.read_progress, 0) >= :finished_threshold AS finished
  FROM book
  WHERE book.read_at IS NOT NULL AND book.deleted_at IS NULL
    AND EXISTS(SELECT 1 FROM book_dir placement
               WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
                 AND (:downloaded_only = 0 OR placement.is_downloaded = 1))
)
SELECT row_id FROM (
    SELECT row_id, finished, ROW_NUMBER() OVER (
        PARTITION BY finished
        ORDER BY read_at DESC, search_title, title, content_hash
    ) AS position
    FROM read_books
)
WHERE position <= :limit
ORDER BY finished, position;

-- name: most_progress_books?
-- param: limit: i32
-- param: downloaded_only: i32
-- param: finished_threshold: f64
SELECT book.row_id
FROM book
WHERE book.deleted_at IS NULL
  AND EXISTS(SELECT 1 FROM book_dir placement
             WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
               AND (:downloaded_only = 0 OR placement.is_downloaded = 1))
  AND COALESCE(book.read_progress, 0) > 0
  AND COALESCE(book.read_progress, 0) < :finished_threshold
ORDER BY book.read_progress DESC, book.search_title, book.title, book.content_hash
LIMIT :limit;

-- name: recently_added_books?
-- param: limit: i32
-- param: downloaded_only: i32
SELECT book.row_id
FROM book
WHERE book.deleted_at IS NULL
  AND EXISTS(SELECT 1 FROM book_dir placement
             WHERE placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
               AND (:downloaded_only = 0 OR placement.is_downloaded = 1))
ORDER BY COALESCE(book.added_at, 0) DESC, LOWER(COALESCE(book.title, '')), book.content_hash
LIMIT :limit;
