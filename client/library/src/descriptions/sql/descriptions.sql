-- Canonical long-description storage and its book-level completion marker.

-- name: set_description!
-- param: content_hash: &str
-- param: description: &str
INSERT INTO book_description(book_row_id, description)
SELECT row_id, :description FROM book WHERE content_hash = :content_hash
  AND (:description <> '' OR EXISTS(SELECT 1 FROM book_description WHERE book_row_id = book.row_id))
ON CONFLICT(book_row_id) DO UPDATE SET description = excluded.description
WHERE book_description.description IS NOT excluded.description;

-- name: mark_description_scanned!
-- param: content_hash: &str
UPDATE book SET description_scanned = 1
WHERE content_hash = :content_hash AND description_scanned IS NOT 1;
