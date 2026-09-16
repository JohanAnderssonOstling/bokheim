-- Resolves local or synchronized book placements.

-- name: get_book_dir_entry_by_path?
-- param: file_path: &str
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
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
    trash_origin_dir_id = NULL,
    local_hash = excluded.local_hash,
    is_downloaded = max(book_dir.is_downloaded, excluded.is_downloaded);

-- name: apply_remote_book_dir_removed!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: origin_folder_id: Option<&str>
-- Upserts a tombstone (deleted_at set) even if the placement was never seen
-- locally, so a later older wall-clock version add still loses the LWW comparison.
INSERT INTO book_dir (dir_id, book_row_id, file_name, local_hash, last_scan, is_downloaded, deleted_at, trash_origin_dir_id)
VALUES (
    :dir_id,
    (SELECT row_id FROM book WHERE content_hash=:content_hash),
    COALESCE((SELECT file_name FROM book_dir WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash)), :content_hash),
    '',
    0,
    0,
    1,
    :origin_folder_id
)
ON CONFLICT(dir_id, book_row_id) DO UPDATE SET
    deleted_at = 1,
    trash_origin_dir_id = excluded.trash_origin_dir_id;

-- name: set_book_downloaded!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: is_downloaded: i32
UPDATE book_dir
SET is_downloaded = :is_downloaded
WHERE dir_id = :dir_id AND book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
  AND deleted_at IS NULL;

-- name: set_book_hash_downloaded!
-- param: content_hash: &str
-- param: is_downloaded: i32
UPDATE book_dir
SET is_downloaded = :is_downloaded
WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
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
WITH RECURSIVE ancestors(id) AS (
    SELECT :parent_id
    UNION
    SELECT d.parent_id
    FROM dir d JOIN ancestors ON d.id=ancestors.id
    WHERE d.id!='00000000-0000-0000-0000-000000000000'
)
SELECT EXISTS(SELECT 1 FROM ancestors WHERE id=:directory_id);
