-- Independent remote presence, local references, and durable transfer intent.
-- name: get_book_materialization_metadata?
-- param: content_hash: &str
SELECT COALESCE(title, ''), publication_metadata, contributors_for_book(row_id), format
FROM book
WHERE content_hash = :content_hash;

-- name: book_dir_file_name_claimed?
-- param: dir_id: &str
-- param: file_name: &str
-- param: content_hash: &str
SELECT EXISTS(
    SELECT 1
    FROM book_dir
    WHERE dir_id = :dir_id
      AND file_name = :file_name
      AND deleted_at IS NULL
      AND book_row_id <> (SELECT row_id FROM book WHERE content_hash=:content_hash)
);

-- name: set_book_dir_file_name!
-- param: file_name: &str
-- param: dir_id: &str
-- param: content_hash: &str
UPDATE book_dir
SET file_name = :file_name
WHERE dir_id = :dir_id
  AND book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
  AND deleted_at IS NULL;

-- name: upsert_blob_local_ref!
-- param: kind: &str
-- param: hash: &str
-- param: path: &str
-- param: exists_flag: i32
-- param: last_seen: i64
INSERT INTO blob_local_ref(kind, hash, path, exists_flag, last_seen)
VALUES (:kind, :hash, :path, :exists_flag, :last_seen)
ON CONFLICT(kind, hash, path) DO UPDATE SET
    exists_flag = :exists_flag,
    last_seen = :last_seen;

-- name: record_remote_asset!
-- param: kind: &str
-- param: hash: &str
INSERT INTO remote_asset(kind, hash)
VALUES (:kind, :hash)
ON CONFLICT(kind, hash) DO NOTHING;

-- name: forget_remote_asset!
-- param: kind: &str
-- param: hash: &str
DELETE FROM remote_asset
WHERE kind = :kind
  AND hash = :hash;

-- name: get_remote_asset_hashes?
-- param: kind: &str
SELECT hash
FROM remote_asset
WHERE kind = :kind;

-- name: get_locally_present_blob_hashes?
-- param: kind: &str
SELECT DISTINCT hash
FROM blob_local_ref
WHERE kind = :kind
  AND exists_flag = 1
UNION
SELECT hash
FROM local_blob
WHERE kind = :kind;

-- name: get_blob_local_presence?
-- param: kind: &str
-- param: hash: &str
SELECT EXISTS(
    SELECT 1 FROM blob_local_ref
    WHERE kind = :kind AND hash = :hash AND exists_flag = 1
    UNION ALL
    SELECT 1 FROM local_blob
    WHERE kind = :kind AND hash = :hash
);

-- name: get_blob_local_path?
-- param: kind: &str
-- param: hash: &str
SELECT path
FROM blob_local_ref
WHERE kind = :kind
  AND hash = :hash
  AND exists_flag = 1
ORDER BY last_seen DESC
LIMIT 1;

-- name: get_blob_local_paths?
-- param: kind: &str
-- param: hash: &str
SELECT path
FROM blob_local_ref
WHERE kind = :kind
  AND hash = :hash
  AND exists_flag = 1
ORDER BY last_seen DESC, path ASC;

-- name: add_download_request!
-- param: target_kind: &str
-- param: target_key: &str
-- param: origin: &str
-- param: created_at: i64
INSERT INTO download_requests(target_kind, target_key, origin, created_at)
VALUES (:target_kind, :target_key, :origin, :created_at)
ON CONFLICT(target_kind, target_key) DO UPDATE SET
    origin = CASE WHEN excluded.origin = 'user_initiated' THEN 'user_initiated' ELSE download_requests.origin END;

-- name: get_download_requests?
-- param: target_kind: &str
SELECT target_key, origin FROM download_requests
WHERE target_kind=:target_kind ORDER BY created_at, target_key;

-- name: complete_download_request!
-- param: target_kind: &str
-- param: target_key: &str
DELETE FROM download_requests
WHERE target_kind=:target_kind AND target_key=:target_key;

-- name: get_blob_any_path?
-- param: kind: &str
-- param: hash: &str
SELECT path
FROM blob_local_ref
WHERE kind = :kind
  AND hash = :hash
ORDER BY exists_flag DESC, last_seen DESC, path ASC
LIMIT 1;

-- name: get_book_paths_by_content_hash?
-- param: content_hash: &str
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
SELECT
    book.content_hash,
    CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END AS file_path
FROM book_dir bd
JOIN book ON book.row_id = bd.book_row_id
JOIN dir_paths ON bd.dir_id = dir_paths.id
WHERE book.content_hash = :content_hash
  AND bd.deleted_at IS NULL
ORDER BY file_path ASC;

-- name: get_live_dir_paths?
WITH RECURSIVE paths(id, path) AS (
    SELECT id, '' FROM dir WHERE id='00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, CASE WHEN paths.path='' THEN d.name ELSE paths.path || '/' || d.name END
    FROM dir d JOIN paths ON d.parent_id=paths.id
    WHERE d.id!='00000000-0000-0000-0000-000000000000' AND d.deleted_at IS NULL
)
SELECT id, path FROM paths WHERE id!='00000000-0000-0000-0000-000000000000';

-- name: book_placement_expects_hash?
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
-- param: content_hash: &str
SELECT EXISTS(
    SELECT 1 FROM book_dir
    JOIN book ON book.row_id = book_dir.book_row_id
    WHERE book_dir.dir_id=:dir_id AND book.content_hash=:content_hash AND book_dir.file_name=:file_name
      AND book.content_hash=:content_hash AND book_dir.deleted_at IS NULL
);
