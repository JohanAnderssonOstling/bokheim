-- Durable catalogue-publication ownership relations.

-- name: upsert_catalog_publication_book!
-- param: source_id: &str
-- param: edition_id: &str
-- param: work_id: &str
-- param: content_hash: &str
INSERT INTO catalog_publication_book(source_id, edition_id, work_id, book_row_id)
VALUES(
    :source_id,
    :edition_id,
    :work_id,
    (SELECT row_id FROM book WHERE content_hash=:content_hash)
)
ON CONFLICT(source_id, edition_id, work_id) DO UPDATE SET
    book_row_id=excluded.book_row_id
WHERE catalog_publication_book.book_row_id IS NOT excluded.book_row_id;
