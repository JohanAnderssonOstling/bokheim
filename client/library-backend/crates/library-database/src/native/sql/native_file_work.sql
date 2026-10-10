-- Native file reconciliation snapshots and acknowledgements.

-- name: native_directory_replay_work?
SELECT id, dir_id FROM local_directory_work ORDER BY id;

-- name: native_book_replay_work_ids?
SELECT id FROM local_book_work ORDER BY id;

-- name: native_complete_directory_replay!
-- param: ids: &str
DELETE FROM local_directory_work WHERE id IN (SELECT value FROM json_each(:ids));

-- name: native_complete_book_replay!
-- param: ids: &str
DELETE FROM local_book_work WHERE id IN (SELECT value FROM json_each(:ids))
AND NOT EXISTS(SELECT 1 FROM local_file_work f WHERE f.content_hash=local_book_work.content_hash)
AND NOT EXISTS(SELECT 1 FROM local_directory_work);

-- name: native_directory_work_ids?
SELECT dir_id FROM local_directory_work ORDER BY id;

-- name: local_book_work_hashes?
SELECT content_hash FROM local_book_work ORDER BY id;

-- name: clear_local_directory_work!
DELETE FROM local_directory_work;

-- name: native_book_work_directory_ids?
SELECT DISTINCT bd.dir_id
FROM local_book_work work
JOIN book book ON book.content_hash = work.content_hash
JOIN book_dir bd ON bd.book_row_id = book.row_id AND bd.deleted_at IS NULL
ORDER BY bd.dir_id;

-- name: native_directory_snapshot?
-- param: dir_id: &str
SELECT CASE WHEN projection.relative_path IS NOT NULL THEN '/' || ltrim(projection.relative_path, '/') END,
       directory.parent_id, directory.name
FROM dir directory
LEFT JOIN local_directory_projection projection ON projection.dir_id = directory.id
WHERE directory.id = :dir_id AND directory.deleted_at IS NULL;

-- name: native_directory_sibling_names?
-- param: parent_id: &str
SELECT name FROM dir WHERE parent_id = :parent_id AND deleted_at IS NULL;

-- name: native_directory_rename_conflict ->
-- param: dir_id: &str
-- param: name: &str
SELECT EXISTS(
    SELECT 1 FROM dir
    WHERE parent_id = (SELECT parent_id FROM dir WHERE id = :dir_id)
      AND id <> :dir_id AND name = :name AND deleted_at IS NULL
);

-- name: native_directory_queue_work!
-- param: dir_id: &str
INSERT INTO local_directory_work(dir_id) VALUES (:dir_id)
ON CONFLICT(dir_id) DO UPDATE SET id = excluded.id;

-- name: native_move_projection_paths!
-- param: prefix: &str
-- param: replacement: &str
-- param: upper: &str
UPDATE local_file_projection
SET relative_path = :replacement || substr(relative_path, length(:prefix) + 1)
WHERE relative_path >= :prefix AND relative_path < :upper;

-- name: native_move_directory_projection_paths!
-- param: prefix: &str
-- param: replacement: &str
-- param: upper: &str
UPDATE local_directory_projection
SET relative_path = :replacement || substr(relative_path, length(:prefix) + 1)
WHERE relative_path >= :prefix AND relative_path < :upper;

-- name: native_set_directory_projection!
-- param: dir_id: &str
-- param: relative_path: &str
INSERT INTO local_directory_projection(dir_id, relative_path) VALUES (:dir_id, :relative_path)
ON CONFLICT(dir_id) DO UPDATE SET relative_path = excluded.relative_path;

-- name: requested_purge_books?
-- param: after: Option<&ContentHash>
SELECT content_hash
FROM local_purge_work
WHERE content_hash > COALESCE(:after,X'')
ORDER BY content_hash
LIMIT 32;

-- name: purge_book_is_retired ->
-- param: content_hash: &ContentHash
SELECT NOT EXISTS(SELECT 1 FROM book WHERE content_hash = :content_hash);

-- name: acknowledge_purge_book!
-- param: content_hash: &ContentHash
-- param: work_id: i64
DELETE FROM local_purge_work
WHERE content_hash = :content_hash AND id = :work_id;

-- Native book staging and conditional publish.

-- name: native_rename_book!
-- param: dir_id: &str
-- param: content_hash: &ContentHash
-- param: name: &str
UPDATE book_dir SET file_name=:name WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: native_file_work_rows?
SELECT id,operation,content_hash,relative_path FROM local_file_work ORDER BY id;

-- Lazy file-work resolution: current live placements for a hash; Rust adds
-- each one's path. The queued path is only a hint; a directory
-- move after queueing must not strand the job.
-- name: native_live_placement_paths?
-- param: content_hash: &ContentHash
SELECT bd.dir_id, bd.file_name FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id WHERE book.content_hash=:content_hash AND bd.deleted_at IS NULL AND book.deleted_at IS NULL;

-- Last materialized locations for a hash; trash work for a retired book
-- resolves here instead of the stale queued path.
-- name: native_file_projection_paths?
-- param: content_hash: &ContentHash
SELECT dir_id, relative_path FROM local_file_projection WHERE content_hash=:content_hash ORDER BY relative_path;

-- name: native_file_is_current ->
-- param: id: i64
-- param: operation: &str
-- param: content_hash: &ContentHash
-- param: relative_path: &str
SELECT EXISTS(SELECT 1 FROM local_file_work WHERE id=:id AND operation=:operation AND content_hash=:content_hash AND relative_path=:relative_path);

-- name: native_request_thumbnail!
-- param: content_hash: &ContentHash
INSERT INTO book_transfer(content_hash,cover_state) SELECT content_hash,'pending' FROM book WHERE content_hash=:content_hash AND deleted_at IS NULL
ON CONFLICT(content_hash) DO UPDATE SET cover_state='pending' WHERE cover_state IS NULL OR cover_state='ready';

-- name: native_delete_file_work!
-- param: id: i64
DELETE FROM local_file_work WHERE id=:id;

-- name: native_record_remote_book!
-- param: content_hash: &ContentHash
INSERT INTO book_transfer(content_hash,server_has_book) SELECT content_hash,1 FROM book WHERE content_hash=:content_hash
ON CONFLICT(content_hash) DO UPDATE SET server_has_book=1;

-- name: native_queue_book_work!
-- param: content_hash: &ContentHash
INSERT INTO local_book_work(content_hash) VALUES(:content_hash) ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;

-- name: native_has_pending_purge_work?
SELECT EXISTS(SELECT 1 FROM local_purge_work);

-- name: native_move_file_work_paths!
-- param: prefix: &str
-- param: replacement: &str
-- param: upper: &str
UPDATE OR REPLACE local_file_work
SET relative_path = :replacement || substr(relative_path, length(:prefix) + 1)
WHERE relative_path >= :prefix AND relative_path < :upper;

-- name: native_has_pending_file_work ->
SELECT EXISTS(SELECT 1 FROM local_directory_work)
    OR EXISTS(SELECT 1 FROM local_file_work)
    OR EXISTS(SELECT 1 FROM local_book_work);
