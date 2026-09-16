-- Independent remote presence and durable transfer intent.

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
