-- Bookmarks are immutable local records: they are created once and can only
-- transition to a tombstone. Remote replay remains handled separately.

-- name: get_bookmarks?
-- param: book_id: &str
SELECT id, book_id, cfi, label, created_at, modified_at
FROM bookmark
WHERE book_id=:book_id AND deleted_at IS NULL
ORDER BY created_at DESC, id;

-- name: create_bookmark!
-- param: id: &str
-- param: book_id: &str
-- param: cfi: &str
-- param: label: &str
-- param: created_at: i64
INSERT INTO bookmark(id,book_id,cfi,label,created_at,modified_at,deleted_at)
VALUES(:id,:book_id,:cfi,:label,:created_at,:created_at,NULL);

-- name: get_live_bookmark?
-- param: id: &str
SELECT book_id,cfi,label,created_at
FROM bookmark
WHERE id=:id AND deleted_at IS NULL;

-- name: delete_bookmark!
-- param: id: &str
-- param: deleted_at: i64
UPDATE bookmark
SET modified_at=:deleted_at, deleted_at=:deleted_at
WHERE id=:id AND deleted_at IS NULL;
