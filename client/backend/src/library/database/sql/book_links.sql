-- Typed persistence for semantic relationships between books.

-- name: get_book_links?
-- param: content_hash: &str
SELECT book_link.id, source.content_hash, target.content_hash, relation, note, created_at, modified_at
FROM book_link
JOIN book source ON source.row_id=book_link.source_book_row_id
JOIN book target ON target.row_id=book_link.target_book_row_id
WHERE (source.content_hash=:content_hash OR target.content_hash=:content_hash)
  AND book_link.deleted_at IS NULL
ORDER BY modified_at DESC, book_link.id;

-- name: live_book_exists?
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=:content_hash AND deleted_at IS NULL);

-- name: get_book_link_by_id?
-- param: id: &str
SELECT book_link.id, source.content_hash, target.content_hash, relation, note, created_at, modified_at
FROM book_link
JOIN book source ON source.row_id=book_link.source_book_row_id
JOIN book target ON target.row_id=book_link.target_book_row_id
WHERE book_link.id=:id;

-- name: upsert_book_link!
-- param: id: &str
-- param: source_content_hash: &str
-- param: target_content_hash: &str
-- param: relation: &str
-- param: note: &str
-- param: created_at: i64
-- param: modified_at: i64
INSERT INTO book_link(
    id,source_book_row_id,target_book_row_id,relation,note,created_at,modified_at,deleted_at
)
VALUES(
    :id,
    (SELECT row_id FROM book WHERE content_hash=:source_content_hash),
    (SELECT row_id FROM book WHERE content_hash=:target_content_hash),
    :relation,:note,:created_at,:modified_at,NULL
)
ON CONFLICT(id) DO UPDATE SET
    source_book_row_id=excluded.source_book_row_id,
    target_book_row_id=excluded.target_book_row_id,
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
