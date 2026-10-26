-- Import commit writes.

-- name: has_import_inspection_select?
-- param: content_hash: &BookId
-- param: format: i64
-- param: version: i64
SELECT EXISTS(
    SELECT 1 FROM book_local_state i JOIN book b USING(content_hash)
    WHERE i.content_hash = :content_hash AND i.inspection_format = :format AND i.inspection_version = :version
      AND b.format = :format AND b.deleted_at IS NULL
);

-- name: provisional_author_identities?
SELECT id, stable_id, name
FROM author_identity
WHERE is_provisional=1
ORDER BY id;

-- name: record_import_inspection!
-- param: content_hash: &BookId
-- param: format: i64
-- param: version: i64
INSERT INTO book_local_state(content_hash, inspection_format, inspection_version)
VALUES (:content_hash, :format, :version)
ON CONFLICT(content_hash) DO UPDATE SET inspection_format = excluded.inspection_format, inspection_version = excluded.inspection_version;

-- name: restore_candidates_select?
-- param: content_hash: &BookId
SELECT bd.dir_id,bd.file_name,p.relative_path
             FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id
             LEFT JOIN local_file_projection p ON p.content_hash=b.content_hash AND p.dir_id=bd.dir_id
             WHERE b.content_hash=:content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL;

-- name: scan_seq_update!
-- param: scan_seq: i64
INSERT INTO sync_metadata(singleton, scan_seq) VALUES (1, :scan_seq) ON CONFLICT(singleton) DO UPDATE SET scan_seq = excluded.scan_seq;

-- name: set_description!
-- param: content_hash: &BookId
-- param: description: &str
INSERT INTO book_description(book_row_id, description)
SELECT row_id, :description FROM book WHERE content_hash = :content_hash
  AND (:description <> '' OR EXISTS(SELECT 1 FROM book_description WHERE book_row_id = book.row_id))
ON CONFLICT(book_row_id) DO UPDATE SET description = excluded.description
WHERE book_description.description IS NOT excluded.description;

