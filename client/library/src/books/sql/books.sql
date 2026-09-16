-- Core book, directory, and local placement write operations.

-- name: insert_book!
-- param: id:   &str
-- param: title: &str
-- param: timestamp: i64
-- param: format: &str
INSERT INTO book    (content_hash, title, read_pos, read_progress, added_at, format, deleted_at, purged_at)
VALUES              (:id,:title, NULL, 0, :timestamp, :format, NULL, NULL)
ON CONFLICT(content_hash) DO UPDATE SET
    title = COALESCE(book.title, excluded.title),
    added_at = excluded.added_at,
    deleted_at = NULL,
    purged_at = NULL,
    trash_origin_dir_id = NULL;

-- name: apply_remote_book_deleted!
-- param: id: &str
-- param: timestamp: i64
-- param: origin_folder_id: Option<&str>
-- Upsert a materialized tombstone even when this device has never observed the
-- book. Its winning version is retained independently in sync_state_version.
INSERT INTO book (content_hash, added_at, deleted_at, trash_origin_dir_id)
VALUES (:id, :timestamp, :timestamp, :origin_folder_id)
ON CONFLICT(content_hash) DO UPDATE SET
    deleted_at = excluded.deleted_at,
    purged_at = NULL,
    trash_origin_dir_id = :origin_folder_id;

-- name: apply_remote_book_purged!
-- param: id: &str
-- param: timestamp: i64
INSERT INTO book (content_hash, added_at, deleted_at, purged_at)
VALUES (:id, :timestamp, :timestamp, :timestamp)
ON CONFLICT(content_hash) DO UPDATE SET
    deleted_at = excluded.deleted_at,
    purged_at = excluded.purged_at,
    trash_origin_dir_id = NULL;

-- name: first_book_insert!
-- param: id: &str
-- param: timestamp: i64
-- Placeholder row for a book this device knows only by reference (a placement or
-- reader-state mutation naming it). Format stays empty until a lifecycle
-- mutation or a local scan observes the actual file; every caller applies remote
-- state, so the empty value never reaches a local outbox trigger.
INSERT OR IGNORE INTO book    (content_hash, added_at)
VALUES              (:id, :timestamp);

-- name: insert_scanned_book!
-- param: id: &str
-- param: timestamp: i64
-- param: format: &str
-- A book observed on this device's disk but carrying no readable metadata. The
-- format comes from the file itself, so unlike first_book_insert this row is
-- complete enough for the lifecycle trigger to encode.
INSERT INTO book    (content_hash, added_at, format)
VALUES              (:id, :timestamp, :format)
ON CONFLICT(content_hash) DO UPDATE SET
    format = excluded.format;

-- name: upsert_dir!
-- param: id: &str
-- param: parent_id: &str
-- param: name: &str
INSERT INTO dir (id, parent_id, name, intent_parent_id, intent_name, intent_lifecycle, deleted_at, purged_at)
VALUES (:id, :parent_id, :name, :parent_id, :name, 0, NULL, NULL)
ON CONFLICT(id) DO UPDATE SET
    parent_id = excluded.parent_id,
    name = excluded.name,
    intent_parent_id = excluded.parent_id,
    intent_name = excluded.name,
    intent_lifecycle = 0,
    deleted_at = NULL,
    purged_at = NULL;

-- name: get_dir_path_components?
-- param: dir_id: &str
WITH RECURSIVE path_chain(id, parent_id, name, level) AS (
    SELECT id, parent_id, name, 0 AS level
    FROM dir
    WHERE id = :dir_id AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name, pc.level + 1
    FROM dir d
    JOIN path_chain pc ON pc.parent_id = d.id
    WHERE d.deleted_at IS NULL AND d.id != '00000000-0000-0000-0000-000000000000'
)
SELECT id, name
FROM path_chain
WHERE id != '00000000-0000-0000-0000-000000000000'
ORDER BY level DESC;


-- name: get_local_fingerprint_book_dirs?
SELECT placement.local_hash, book.content_hash, placement.dir_id, placement.file_name, book.content_hash,
       book.description_scanned = 0 AS description_missing
FROM book_dir placement
JOIN book ON book.row_id = placement.book_row_id
WHERE placement.deleted_at IS NULL AND placement.local_hash != '' AND book.content_hash != '';

-- name: insert_book_dir!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
-- param: local_hash: &str
-- param: last_scan: i64
-- param: is_downloaded: i32
INSERT INTO book_dir    (dir_id, book_row_id, file_name, local_hash, last_scan, is_downloaded, deleted_at)
VALUES                  (:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,:local_hash,:last_scan,:is_downloaded,NULL)
ON CONFLICT(dir_id, book_row_id) DO UPDATE SET
    deleted_at = NULL,
    trash_origin_dir_id = NULL,
    -- max() so a remote apply (last_scan 0) can't lower a live scan stamp and
    -- make the next sweep delete a present file.
    last_scan = max(book_dir.last_scan, excluded.last_scan),
    is_downloaded = max(book_dir.is_downloaded, excluded.is_downloaded),
    local_hash = excluded.local_hash;
