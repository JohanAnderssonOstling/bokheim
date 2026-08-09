-- Resolves and materializes local or synchronized physical book placements.

-- name: get_book_dir_entry_by_path?
-- param: file_path: &str
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '__root__' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name,
        CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END
    FROM dir d
    JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id
    WHERE d.deleted_at IS NULL
)
SELECT book.content_hash, bd.dir_id, bd.file_name
FROM book_dir bd
JOIN book ON book.row_id = bd.book_row_id
JOIN dir_paths ON bd.dir_id = dir_paths.id
WHERE CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END = :file_path
  AND bd.deleted_at IS NULL
LIMIT 1;

-- name: resolve_removed_book_dir?
-- param: dir_id: &str
-- param: content_hash: &str
-- Resolves a tombstoned placement's file path AND content hash. Returns nothing
-- unless the placement is currently deleted, so a placement an add resurrected
-- later in the same pull batch is left alone. The content hash lets the caller
-- relocate the file to another live placement of the same content (a move)
-- instead of deleting it.
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '__root__' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name,
        CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END
    FROM dir d
    JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id
    WHERE d.deleted_at IS NULL
)
SELECT
    CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END AS file_path,
    book.content_hash
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
JOIN book ON book.row_id = bd.book_row_id
WHERE bd.dir_id = :dir_id AND book.content_hash = :content_hash
  AND bd.deleted_at IS NOT NULL
LIMIT 1;

-- Remote materialization of a placement after the shared version gate.
-- change_origin is 'remote' during apply, so the book_dir triggers don't fire.
-- name: apply_remote_book_dir_added!
-- param: dir_id: &str
-- param: content_hash: &str
INSERT INTO book_dir (dir_id, book_row_id, file_name, local_hash, last_scan, is_downloaded, deleted_at)
VALUES (
    :dir_id,
    (SELECT row_id FROM book WHERE content_hash=:content_hash),
    COALESCE((SELECT file_name FROM book_dir WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash)), :content_hash),
    '',
    0,
    0,
    NULL
)
ON CONFLICT(dir_id, book_row_id) DO UPDATE SET
    deleted_at = NULL,
    local_hash = excluded.local_hash,
    is_downloaded = max(book_dir.is_downloaded, excluded.is_downloaded);

-- name: apply_remote_book_dir_removed!
-- param: dir_id: &str
-- param: content_hash: &str
-- Upserts a tombstone (deleted_at set) even if the placement was never seen
-- locally, so a later older wall-clock version add still loses the LWW comparison.
INSERT INTO book_dir (dir_id, book_row_id, file_name, local_hash, last_scan, is_downloaded, deleted_at)
VALUES (
    :dir_id,
    (SELECT row_id FROM book WHERE content_hash=:content_hash),
    COALESCE((SELECT file_name FROM book_dir WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash)), :content_hash),
    '',
    0,
    0,
    1
)
ON CONFLICT(dir_id, book_row_id) DO UPDATE SET
    deleted_at = 1;

-- name: set_book_downloaded!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: is_downloaded: i32
UPDATE book_dir
SET is_downloaded = :is_downloaded
WHERE dir_id = :dir_id AND book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
  AND deleted_at IS NULL;

-- name: update_book_dir_local_hash!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: local_hash: &str
UPDATE book_dir
SET local_hash = :local_hash
WHERE dir_id = :dir_id AND book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
  AND deleted_at IS NULL;

-- name: directory_move_closes_cycle?
-- param: parent_id: &str
-- param: directory_id: &str
WITH RECURSIVE ancestors(id, depth) AS (
    SELECT :parent_id, 0
    UNION ALL
    SELECT d.parent_id, ancestors.depth + 1
    FROM dir d JOIN ancestors ON d.id=ancestors.id
    WHERE d.deleted_at IS NULL AND d.id!='__root__' AND ancestors.depth < 10000
)
SELECT EXISTS(SELECT 1 FROM ancestors WHERE id=:directory_id);
