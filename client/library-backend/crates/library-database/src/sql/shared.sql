-- Shared statements used by multiple database features.

-- name: shared_owner_replica_id?
SELECT replica_id FROM sync_metadata WHERE singleton = 1;


-- name: shared_owner_last_successful_sync_at?
SELECT CAST(last_successful_sync_at AS TEXT) FROM sync_metadata WHERE singleton = 1;


-- name: shared_owner_remote_library_name?
SELECT remote_library_name FROM sync_metadata WHERE singleton = 1;


-- name: shared_owner_server_library_created?
SELECT server_library_created FROM sync_metadata WHERE singleton = 1;


-- Thumbnail recovery and remote availability.


-- name: shared_request_thumbnail!
-- param: content_hash: &ContentHash
INSERT INTO book_transfer(content_hash, cover_state)
SELECT content_hash, 'pending' FROM book WHERE content_hash = :content_hash AND deleted_at IS NULL
ON CONFLICT(content_hash) DO UPDATE SET cover_state = 'pending';


-- name: shared_scanner_sequence?
SELECT CAST(scan_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;


-- name: shared_scanner_enqueue_upload!
-- param: content_hash: &ContentHash
-- param: size_bytes: i64
-- Queues the bytes under a new upload id: one past any id still stored, so
-- an earlier upload's preparation never matches it.
INSERT INTO book_transfer(content_hash, upload_id, upload_size)
SELECT content_hash, (SELECT COALESCE(MAX(MAX(COALESCE(upload_id, 0), COALESCE(upload_prepared, 0))), 0) + 1 FROM book_transfer), :size_bytes
FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET upload_id = excluded.upload_id, upload_size = excluded.upload_size;



-- name: shared_scanner_publish_projection!
-- param: content_hash: &ContentHash
-- param: dir_id: &str
-- param: relative_path: &str
INSERT INTO local_file_projection(content_hash,dir_id,relative_path) VALUES(:content_hash,:dir_id,:relative_path) ON CONFLICT(content_hash,dir_id) DO UPDATE SET relative_path=excluded.relative_path;


-- name: shared_occupied_file_names?
-- param: destination: &str
SELECT file_name FROM book_dir WHERE dir_id = :destination AND deleted_at IS NULL;


-- name: shared_set_book_hash_downloaded!
-- param: content_hash: &ContentHash
-- param: is_downloaded: i32
UPDATE book_dir
SET is_downloaded = :is_downloaded
WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
  AND deleted_at IS NULL;


-- name: shared_placements_queue_directory_work!
-- param: dir_id: &str
INSERT OR REPLACE INTO local_directory_work(dir_id) VALUES(:dir_id);


-- name: shared_placements_queue_file_work!
-- param: operation: &str
-- param: content_hash: &ContentHash
-- param: relative_path: &str
INSERT OR IGNORE INTO local_file_work(operation,content_hash,relative_path) VALUES(:operation,:content_hash,:relative_path);


-- name: shared_cancel_book_materialization!
-- param: content_hash: &ContentHash
DELETE FROM local_file_work WHERE content_hash=:content_hash AND operation='materialize';


-- name: shared_purge_book_record_insert!
-- param: content_hash: &ContentHash
INSERT INTO local_purge_work(content_hash) VALUES(:content_hash)
ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;


-- name: shared_cursor_lifecycle_select?
SELECT CAST(last_pull_lifecycle_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;


-- name: shared_cursor_field_select?
SELECT CAST(last_pull_field_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;


-- name: shared_apply_remote_book_purged_select?
-- param: content_hash: &ContentHash
SELECT row_id FROM book WHERE content_hash=:content_hash;


-- name: shared_authors_insert_author_identity!
-- param: stable_id: &str
-- param: name: &str
INSERT INTO author_identity(stable_id, name)
VALUES(:stable_id, :name);


-- name: shared_transfer_complete_download!
-- param: content_hash: &ContentHash
UPDATE book_transfer SET book_download = NULL, book_download_at = NULL WHERE content_hash = :content_hash;


-- name: shared_bytes_seen?
-- param: content_hash: &ContentHash
SELECT EXISTS(SELECT 1 FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id
              WHERE b.content_hash=:content_hash AND bd.size_bytes IS NOT NULL);

-- name: shared_record_size!
-- param: dir_id: &str
-- param: content_hash: &ContentHash
-- param: size_bytes: i64
UPDATE book_dir SET size_bytes=:size_bytes
WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: shared_request_book_download!
-- param: content_hash: &ContentHash
-- param: origin: &str
-- param: requested_at: i64
-- A user's request outranks a background one; the first request's time is kept.
INSERT INTO book_transfer(content_hash, book_download, book_download_at)
SELECT content_hash, :origin, :requested_at FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET
    book_download = CASE WHEN excluded.book_download = 'user_initiated' OR book_download IS NULL THEN excluded.book_download ELSE book_download END,
    book_download_at = COALESCE(book_download_at, excluded.book_download_at);

-- name: shared_book_format?
-- param: content_hash: &ContentHash
SELECT format FROM book WHERE content_hash = :content_hash;

-- name: shared_audiobook_recording_evidence?
-- param: content_hash: &ContentHash
SELECT recording_json FROM book_local_state WHERE content_hash = :content_hash AND recording_json IS NOT NULL;

-- name: shared_book_name_conflict ->
-- param: dir_id: &str
-- param: file_name_key: &str
-- param: content_hash: &ContentHash
SELECT EXISTS(SELECT 1 FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.dir_id=:dir_id AND portable_name_key(bd.file_name)=:file_name_key AND b.content_hash!=:content_hash AND bd.deleted_at IS NULL);

-- name: shared_live_book_placements?
-- param: content_hash: &ContentHash
-- The book's live placements; Rust turns each into a path.
SELECT bd.dir_id, bd.file_name
FROM book_dir bd
JOIN book b ON b.row_id = bd.book_row_id
WHERE b.content_hash = :content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL;
