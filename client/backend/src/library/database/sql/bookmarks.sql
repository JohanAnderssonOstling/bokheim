-- Bookmarks are immutable local records: they are created once and can only
-- transition to a tombstone. Remote replay remains handled separately.

-- name: get_bookmarks?
-- param: content_hash: &str
SELECT bookmark.id, book.content_hash, cfi, label, created_at, modified_at
FROM bookmark JOIN book ON book.row_id=bookmark.book_row_id
WHERE book.content_hash=:content_hash AND bookmark.deleted_at IS NULL
ORDER BY created_at DESC, bookmark.id;

-- name: create_bookmark!
-- param: id: &str
-- param: content_hash: &str
-- param: cfi: &str
-- param: label: &str
-- param: created_at: i64
INSERT INTO bookmark(id,book_row_id,cfi,label,created_at,modified_at,deleted_at)
VALUES(:id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:cfi,:label,:created_at,:created_at,NULL);

-- name: get_live_bookmark?
-- param: id: &str
SELECT book.content_hash,cfi,label,created_at
FROM bookmark JOIN book ON book.row_id=bookmark.book_row_id
WHERE bookmark.id=:id AND bookmark.deleted_at IS NULL;

-- name: delete_bookmark!
-- param: id: &str
-- param: deleted_at: i64
UPDATE bookmark
SET modified_at=:deleted_at, deleted_at=:deleted_at
WHERE id=:id AND deleted_at IS NULL;
