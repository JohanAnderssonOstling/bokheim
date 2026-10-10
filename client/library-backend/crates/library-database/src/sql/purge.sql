-- Retire book state while retaining lifecycle intent and file cleanup.

-- name: purge_queue_observed_files !
-- param: content_hash: &SqlParam<'_>
INSERT OR IGNORE INTO local_file_work(operation,content_hash,relative_path)
         SELECT 'remove',:content_hash,relative_path FROM local_file_projection WHERE content_hash=:content_hash;

-- name: purge_book_placements ?
-- param: content_hash: &SqlParam<'_>
SELECT bd.dir_id,bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash=:content_hash;

-- name: purge_queue_placement_file !
-- param: content_hash: &SqlParam<'_>
-- param: relative_path: &SqlParam<'_>
INSERT OR IGNORE INTO local_file_work(operation,content_hash,relative_path) VALUES('remove',:content_hash,:relative_path);

-- name: purge_book_publications !
-- param: book_key: &SqlParam<'_>
DELETE FROM sync_outbox WHERE (state_kind,state_key,state_subkey) IN (SELECT state_kind,state_key,state_subkey FROM sync_state_version WHERE book_key=:book_key AND state_kind!='book_lifecycle' UNION ALL SELECT state_kind,content_hash,'' FROM sync_book_record WHERE content_hash=:book_key);

-- name: purge_book_registers !
-- param: book_key: &SqlParam<'_>
DELETE FROM sync_state_version WHERE book_key=:book_key AND state_kind!='book_lifecycle';

-- name: purge_delete_book !
-- param: content_hash: &SqlParam<'_>
DELETE FROM book WHERE content_hash=:content_hash;
