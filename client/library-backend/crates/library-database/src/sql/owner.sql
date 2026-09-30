-- Library ownership, lifecycle, and durable session state.

-- name: owner_set_replica_id!
-- param: value: &str
INSERT INTO sync_metadata(singleton, replica_id) VALUES (1, :value)
ON CONFLICT(singleton) DO UPDATE SET replica_id = excluded.replica_id;

-- name: owner_set_change_origin!
-- param: value: &str
INSERT INTO sync_metadata(singleton, change_origin) VALUES (1, :value)
ON CONFLICT(singleton) DO UPDATE SET change_origin = excluded.change_origin;

-- name: owner_set_remote_library_name!
-- param: value: &str
INSERT INTO sync_metadata(singleton, remote_library_name) VALUES (1, :value)
ON CONFLICT(singleton) DO UPDATE SET remote_library_name = excluded.remote_library_name;

-- name: owner_set_server_library_created!
-- param: value: &str
INSERT INTO sync_metadata(singleton, server_library_created) VALUES (1, :value)
ON CONFLICT(singleton) DO UPDATE SET server_library_created = excluded.server_library_created;

-- name: owner_transfer_history?
SELECT entries_json FROM local_transfer_history WHERE singleton = 1;

-- name: owner_save_transfer_history!
-- param: entries_json: &str
INSERT INTO local_transfer_history(singleton, entries_json) VALUES (1, :entries_json)
ON CONFLICT(singleton) DO UPDATE SET entries_json = excluded.entries_json;

-- name: owner_sync_initialized ->
SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'sync_metadata' AND type = 'table');

-- name: owner_has_unsynced_changes ->
SELECT EXISTS(SELECT 1 FROM sync_outbox LIMIT 1);

-- name: owner_cloud_storage_policy!
-- param: enabled: &str
INSERT INTO sync_metadata(singleton, cloud_storage_enabled) VALUES (1, :enabled)
ON CONFLICT(singleton) DO UPDATE SET cloud_storage_enabled = excluded.cloud_storage_enabled WHERE cloud_storage_enabled IS NOT excluded.cloud_storage_enabled;

-- name: owner_reset_remote_books!
UPDATE book_transfer SET server_has_book = 0 WHERE server_has_book;

-- name: owner_reset_rejected_book_uploads!
UPDATE book_transfer SET upload_rejected_reason = NULL, upload_rejected_at = NULL WHERE upload_rejected_at IS NOT NULL;

-- name: owner_cloud_storage_candidates?
-- Each book's recorded local versions, newest first; Rust keeps the newest
-- one in a live folder and queues it for upload.
SELECT b.content_hash, bd.dir_id, bd.checksum, bd.size_bytes
FROM book_dir bd
JOIN book b ON b.row_id = bd.book_row_id AND b.deleted_at IS NULL
WHERE bd.deleted_at IS NULL AND bd.checksum IS NOT NULL AND bd.size_bytes > 0
ORDER BY bd.checksum_seq DESC;

-- name: owner_queue_book_upload!
-- param: content_hash: &str
-- param: checksum: &str
-- param: size_bytes: i64
INSERT INTO book_transfer(content_hash, upload_id, upload_checksum, upload_size)
SELECT content_hash, (SELECT COALESCE(MAX(MAX(COALESCE(upload_id, 0), COALESCE(upload_prepared, 0))), 0) + 1 FROM book_transfer), :checksum, :size_bytes FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET upload_id = excluded.upload_id, upload_checksum = excluded.upload_checksum, upload_size = excluded.upload_size
WHERE upload_id IS NULL;

-- name: owner_refresh_book_uploads!
-- Every queued upload gets a new id, past any stored one, so it is prepared again.
UPDATE book_transfer SET upload_id = upload_id + (SELECT MAX(MAX(COALESCE(upload_id, 0), COALESCE(upload_prepared, 0))) FROM book_transfer)
WHERE upload_id IS NOT NULL;

-- name: owner_operation_record?
SELECT operation_kind, operation_scan_complete, operation_completed FROM sync_metadata WHERE singleton = 1;

-- name: owner_book_format?
-- param: content_hash: &str
SELECT format FROM book WHERE content_hash = :content_hash;

-- name: taxonomy_attached?
SELECT EXISTS(SELECT 1 FROM pragma_database_list WHERE name='curated');

-- name: taxonomy_attach!
-- param: db_name: &str
ATTACH :db_name AS curated;
