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
-- param: content_hash: &str
INSERT INTO book_transfer(content_hash, cover_state)
SELECT content_hash, 'pending' FROM book WHERE content_hash = :content_hash AND deleted_at IS NULL
ON CONFLICT(content_hash) DO UPDATE SET cover_state = 'pending', cover_retry_after = 0;


-- name: shared_scanner_sequence?
SELECT CAST(scan_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;


-- name: shared_scanner_enqueue_upload!
-- param: content_hash: &str
-- param: checksum: &str
-- param: size_bytes: i64
-- Queues the bytes under a new upload id: one past any id still stored, so
-- an earlier upload's preparation never matches it.
INSERT INTO book_transfer(content_hash, upload_id, upload_checksum, upload_size)
SELECT content_hash, (SELECT COALESCE(MAX(MAX(COALESCE(upload_id, 0), COALESCE(upload_prepared, 0))), 0) + 1 FROM book_transfer), :checksum, :size_bytes
FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET upload_id = excluded.upload_id, upload_checksum = excluded.upload_checksum, upload_size = excluded.upload_size;



-- name: shared_scanner_publish_projection!
-- param: content_hash: &str
-- param: dir_id: &str
-- param: relative_path: &str
INSERT INTO local_file_projection(content_hash,dir_id,relative_path) VALUES(:content_hash,:dir_id,:relative_path) ON CONFLICT(content_hash,dir_id) DO UPDATE SET relative_path=excluded.relative_path;


-- name: shared_occupied_file_names?
-- param: destination: &str
SELECT file_name FROM book_dir WHERE dir_id = :destination AND deleted_at IS NULL;


-- name: shared_set_book_hash_downloaded!
-- param: content_hash: &str
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
-- param: content_hash: &str
-- param: relative_path: &str
INSERT OR IGNORE INTO local_file_work(operation,content_hash,relative_path) VALUES(:operation,:content_hash,:relative_path);


-- name: shared_cancel_book_materialization!
-- param: content_hash: &str
DELETE FROM local_file_work WHERE content_hash=:content_hash AND operation='materialize';


-- name: shared_purge_book_identifier!
-- param: book_row_id: i64
DELETE FROM book_identifier WHERE book_row_id=:book_row_id;


-- name: shared_purge_book_record_insert!
-- param: content_hash: &str
INSERT INTO local_purge_work(content_hash) VALUES(:content_hash)
ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;


-- name: shared_cursor_lifecycle_select?
SELECT CAST(last_pull_lifecycle_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;


-- name: shared_cursor_field_select?
SELECT CAST(last_pull_field_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;


-- name: shared_apply_remote_book_purged_select?
-- param: content_hash: &str
SELECT row_id FROM book WHERE content_hash=:content_hash;


-- name: shared_authors_insert_author_identity!
-- param: stable_id: &str
-- param: normalized_name: &str
INSERT INTO author_identity(stable_id, normalized_name)
VALUES(:stable_id, :normalized_name);


-- name: shared_transfer_complete_download!
-- param: content_hash: &str
UPDATE book_transfer SET book_download = NULL, book_download_at = NULL WHERE content_hash = :content_hash;


-- name: shared_checksum_seen?
-- param: content_hash: &str
-- param: checksum: &str
-- Whether some placement of the book already records these bytes.
SELECT EXISTS(SELECT 1 FROM book_dir bd JOIN book b ON b.row_id = bd.book_row_id
              WHERE b.content_hash = :content_hash AND bd.checksum = :checksum);

-- name: shared_current_checksum_select?
-- param: dir_id: &str
-- param: content_hash: &str
SELECT bd.checksum FROM book_dir bd JOIN book b ON b.row_id = bd.book_row_id
WHERE bd.dir_id = :dir_id AND b.content_hash = :content_hash AND bd.checksum IS NOT NULL;

-- name: shared_record_checksum!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: checksum: &str
-- param: size_bytes: i64
UPDATE book_dir SET checksum = :checksum, size_bytes = :size_bytes, checksum_seq = (SELECT COALESCE(MAX(checksum_seq), 0) + 1 FROM book_dir)
WHERE dir_id = :dir_id AND book_row_id = (SELECT row_id FROM book WHERE content_hash = :content_hash);

-- name: shared_rejected_book_cleanup!
-- param: content_hash: &str
UPDATE book_transfer SET upload_rejected_reason = NULL, upload_rejected_at = NULL WHERE content_hash = :content_hash;


-- name: shared_request_book_download!
-- param: content_hash: &str
-- param: origin: &str
-- param: requested_at: i64
-- A user's request outranks a background one; the first request's time is kept.
INSERT INTO book_transfer(content_hash, book_download, book_download_at)
SELECT content_hash, :origin, :requested_at FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET
    book_download = CASE WHEN excluded.book_download = 'user_initiated' OR book_download IS NULL THEN excluded.book_download ELSE book_download END,
    book_download_at = COALESCE(book_download_at, excluded.book_download_at);

-- name: shared_book_format?
-- param: content_hash: &str
SELECT format FROM book WHERE content_hash = :content_hash;

-- name: shared_audiobook_recording_evidence?
-- param: content_hash: &str
SELECT recording_json FROM book_local_state WHERE content_hash = :content_hash AND recording_json IS NOT NULL;

-- name: shared_book_name_conflict ->
-- param: dir_id: &str
-- param: file_name_key: &str
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.dir_id=:dir_id AND portable_name_key(bd.file_name)=:file_name_key AND b.content_hash!=:content_hash AND bd.deleted_at IS NULL);

-- name: shared_live_book_placements?
-- param: content_hash: &str
-- The book's live placements; Rust turns each into a path.
SELECT bd.dir_id, bd.file_name
FROM book_dir bd
JOIN book b ON b.row_id = bd.book_row_id
WHERE b.content_hash = :content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL;
