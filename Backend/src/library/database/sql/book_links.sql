-- Typed persistence for semantic relationships between books.

-- name: get_book_links?
-- param: book_id: &str
SELECT id, source_book_id, target_book_id, relation, note, created_at, modified_at
FROM book_link
WHERE (source_book_id=:book_id OR target_book_id=:book_id)
  AND deleted_at IS NULL
ORDER BY modified_at DESC, id;

-- name: live_book_exists?
-- param: book_id: &str
SELECT EXISTS(SELECT 1 FROM book WHERE id=:book_id AND deleted_at IS NULL);

-- name: get_book_link_by_id?
-- param: id: &str
SELECT id, source_book_id, target_book_id, relation, note, created_at, modified_at
FROM book_link
WHERE id=:id;

-- name: upsert_book_link!
-- param: id: &str
-- param: source_book_id: &str
-- param: target_book_id: &str
-- param: relation: &str
-- param: note: &str
-- param: created_at: i64
-- param: modified_at: i64
INSERT INTO book_link(
    id,source_book_id,target_book_id,relation,note,created_at,modified_at,deleted_at
)
VALUES(
    :id,:source_book_id,:target_book_id,:relation,:note,:created_at,:modified_at,NULL
)
ON CONFLICT(id) DO UPDATE SET
    source_book_id=excluded.source_book_id,
    target_book_id=excluded.target_book_id,
    relation=excluded.relation,
    note=excluded.note,
    created_at=excluded.created_at,
    modified_at=excluded.modified_at,
    deleted_at=NULL;

-- name: delete_book_link!
-- param: id: &str
-- param: modified_at: i64
UPDATE book_link
SET modified_at=:modified_at, deleted_at=:modified_at
WHERE id=:id;
