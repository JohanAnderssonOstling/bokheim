-- name: get_authors?
-- param: downloaded_only: i32
-- param: author_role: Vec<u8>
-- Authors file under their display name.
SELECT
    author_identity.stable_id AS author_id,
    MIN(credit.name) AS display_name,
    COUNT(DISTINCT book.row_id) AS book_count
FROM author_identity
JOIN book_contributor credit ON credit.author_identity_id = author_identity.id AND credit.role = :author_role
JOIN book ON book.row_id = credit.book_row_id
JOIN book_dir ON book_dir.book_row_id = book.row_id
WHERE book.deleted_at IS NULL
  AND book_dir.deleted_at IS NULL
  AND TRIM(credit.name) != ''
  AND (:downloaded_only = 0 OR EXISTS (
      SELECT 1 FROM book_dir available
      WHERE available.book_row_id = book.row_id
        AND available.deleted_at IS NULL
        AND available.is_downloaded = 1
  ))
GROUP BY author_identity.id, author_identity.stable_id
ORDER BY LOWER(display_name);

-- name: author_shelf_books?
-- param: downloaded_only: i32
-- param: limit: i32
-- param: author_role: Vec<u8>
-- The books an author's shelf carries, most recently read first, then by title,
-- capped per author, as (author, book id) pairs in shelf order. One statement
-- for the whole index rather than a correlated lookup per row, so the cost does
-- not scale with the list.
WITH credited AS (
    -- DISTINCT, not GROUP BY: one book credited twice to the same identity —
    -- two spellings that resolved together — is still one book.
    SELECT DISTINCT
           author_identity.stable_id AS author_id,
           book.row_id AS book_row_id, book.title, book.content_hash, book.read_at
    FROM author_identity
    JOIN book_contributor credit ON credit.author_identity_id = author_identity.id AND credit.role = :author_role
    JOIN book ON book.row_id = credit.book_row_id
    WHERE book.deleted_at IS NULL
      AND EXISTS(SELECT 1 FROM book_dir placement
             WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL
               AND (:downloaded_only=0 OR placement.is_downloaded=1))
)
SELECT author_id, book_row_id
FROM (
    SELECT *,
           ROW_NUMBER() OVER (
               PARTITION BY author_id
               ORDER BY COALESCE(read_at, 0) DESC, LOWER(COALESCE(title, '')), content_hash
           ) AS position
    FROM credited
)
WHERE position <= :limit
ORDER BY author_id, position;
