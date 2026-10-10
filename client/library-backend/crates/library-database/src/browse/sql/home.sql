-- name: recently_read_books?
-- param: limit: i32
-- param: downloaded_only: i32
-- param: finished_threshold: f64
-- The limit applies to books in progress and to finished books separately.
-- A single LIMIT over the whole list let a run of finished books crowd out
-- the ones still being read, which is the list Home exists to show; ranking
-- within each group spends the limit twice in one statement rather than
-- asking for the two lists in two round trips. Each book comes with whether
-- it is finished, which is the shelf it belongs on.
WITH read_books AS (
  SELECT book.row_id, book.search_title, book.title, book.content_hash, book.read_at,
         COALESCE(book.read_progress, 0) >= :finished_threshold AS finished
  FROM book
  WHERE book.read_at IS NOT NULL AND book.deleted_at IS NULL
    AND EXISTS(SELECT 1 FROM library_book_presence presence
             WHERE presence.book_row_id=book.row_id AND (:downloaded_only=0 OR presence.downloaded=1))
)
SELECT row_id, finished FROM (
    SELECT row_id, finished, ROW_NUMBER() OVER (
        PARTITION BY finished
        ORDER BY read_at DESC, search_title, title, content_hash
    ) AS position
    FROM read_books
)
WHERE position <= :limit
ORDER BY finished, position;

-- name: recently_added_books?
-- param: limit: i32
-- param: downloaded_only: i32
SELECT book.row_id
FROM book
WHERE book.deleted_at IS NULL
  AND EXISTS(SELECT 1 FROM library_book_presence presence
             WHERE presence.book_row_id=book.row_id AND (:downloaded_only=0 OR presence.downloaded=1))
ORDER BY COALESCE(book.added_at, 0) DESC, LOWER(COALESCE(book.title, '')), book.content_hash
LIMIT :limit;
