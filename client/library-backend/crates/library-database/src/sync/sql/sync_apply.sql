-- Sync apply: remote change projection.



-- name: activate_remote_directory!
-- param: dir_id: &str
UPDATE dir SET deleted_at=NULL, purged_at=NULL WHERE id=:dir_id;

-- name: apply_remote_book_dir_added!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
INSERT INTO book_dir (dir_id, book_row_id, file_name, local_hash, last_scan, is_downloaded, deleted_at)
VALUES (
    :dir_id,
    (SELECT row_id FROM book WHERE content_hash=:content_hash),
    :file_name,
    '',
    0,
    0,
    NULL
)
ON CONFLICT(dir_id, book_row_id) DO UPDATE SET
    file_name = excluded.file_name,
    deleted_at = NULL,
    trash_origin_dir_id = NULL,
    is_downloaded = max(book_dir.is_downloaded, excluded.is_downloaded);

-- name: apply_remote_book_dir_removed!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: origin_folder_id: Option<&str>
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

-- name: author_identity_by_stable_id?
-- param: stable_id: &str
SELECT id, stable_id FROM author_identity WHERE stable_id=:stable_id;

-- name: directory_projection_rows_select?
SELECT id,parent_id,name,deleted_at IS NOT NULL FROM dir;

-- name: ensure_remote_directory_placeholder!
-- param: dir_id: &str
INSERT OR IGNORE INTO dir(id,parent_id,name,intent_parent_id,intent_name,intent_lifecycle,deleted_at,purged_at)
VALUES(:dir_id,'00000000-0000-0000-0000-000000000000','Pending','00000000-0000-0000-0000-000000000000','Pending',1,1,NULL);

-- name: hide_remote_directories!
UPDATE dir SET deleted_at=1, purged_at=NULL WHERE id != '00000000-0000-0000-0000-000000000000';

-- name: mark_remote_directory_purged!
-- param: dir_id: &str
UPDATE dir SET deleted_at=1, purged_at=1 WHERE id=:dir_id;

-- name: insert_book_identifier!
-- param: book_row_id: i64
-- param: position: i64
-- param: scheme: &str
-- param: value: &str
-- param: canonical_value: Option<&str>
-- param: scope: &str
INSERT INTO book_identifier(book_row_id,position,scheme,value,canonical_value,scope)
VALUES(:book_row_id,:position,:scheme,:value,:canonical_value,:scope);

-- name: insert_book_language!
-- param: book_row_id: i64
-- param: position: i64
-- param: language_tag: &str
INSERT OR IGNORE INTO book_language(book_row_id,position,language_tag) VALUES(:book_row_id,:position,:language_tag);

-- name: insert_book_value!
-- param: book_row_id: i64
-- param: kind: &str
-- param: position: i64
-- param: value: &str
-- param: qualifier: Option<&str>
-- param: secondary_value: Option<&str>
INSERT INTO book_value(book_row_id,kind,position,value,qualifier,secondary_value)
VALUES(:book_row_id,:kind,:position,:value,:qualifier,:secondary_value);

-- name: meta_replace_prepared_projections_delete!
-- param: book_row_id: i64
-- param: kind: &str
DELETE FROM book_value WHERE book_row_id=:book_row_id AND kind=:kind;

-- name: meta_update_book_metadata_state_preserving_ids_update_2!
-- param: content_hash: &str
-- param: title: &str
-- param: subtitle: Option<&str>
-- param: book_metadata: &[u8]
UPDATE book SET title=:title,subtitle=:subtitle,book_metadata=:book_metadata
        WHERE content_hash=:content_hash AND (title IS NOT :title OR subtitle IS NOT :subtitle OR book_metadata IS NOT :book_metadata);

-- name: pdf_reader_metadata_upsert!
-- param: content_hash: &str
-- param: checksum: &str
-- param: metadata: &[u8]
INSERT INTO pdf_reader_metadata VALUES(:content_hash,:checksum,:metadata) ON CONFLICT(content_hash,checksum) DO UPDATE SET metadata=excluded.metadata WHERE metadata IS NOT excluded.metadata;

-- name: project_remote_directory!
-- param: dir_id: &str
-- param: parent_id: &str
-- param: name: &str
UPDATE dir SET parent_id=:parent_id, name=:name WHERE id=:dir_id;

-- name: queue_book_work_batch!
-- param: queue_json: &str
INSERT OR REPLACE INTO local_book_work(content_hash)
SELECT value FROM json_each(:queue_json) ORDER BY CAST(key AS INTEGER);

-- name: queue_book_work_insert!
-- param: content_hash: &str
INSERT OR REPLACE INTO local_book_work(content_hash) VALUES(:content_hash);

-- name: read_sync_state_version_select?
-- param: state_kind: &str
-- param: state_key: &str
-- param: state_subkey: &str
SELECT changed_at,conflict_rank,replica_id,replica_seq,mutation_id FROM sync_state_version
        WHERE state_kind=:state_kind AND state_key=:state_key AND state_subkey=:state_subkey;

-- name: set_remote_directory_lifecycle!
-- param: dir_id: &str
-- param: lifecycle: i32
UPDATE dir SET intent_lifecycle=:lifecycle WHERE id=:dir_id;

-- name: set_remote_directory_name!
-- param: dir_id: &str
-- param: name: &str
UPDATE dir SET intent_name=:name WHERE id=:dir_id;

-- name: set_remote_directory_parent!
-- param: dir_id: &str
-- param: parent_id: &str
UPDATE dir SET intent_parent_id=:parent_id WHERE id=:dir_id;

-- name: reserve_write_lock!
UPDATE main.book SET row_id=row_id WHERE 0;

-- name: outbox_delete_for_ids!
-- param: payload: &str
DELETE FROM sync_outbox WHERE mutation_id IN (SELECT value FROM json_each(:payload));

-- name: change_origin_select?
SELECT change_origin FROM sync_metadata WHERE singleton = 1;

-- name: mark_origin_remote!
INSERT INTO sync_metadata(singleton, change_origin) VALUES (1, 'remote') ON CONFLICT(singleton) DO UPDATE SET change_origin = excluded.change_origin;

-- name: restore_origin!
-- param: origin: &str
INSERT INTO sync_metadata(singleton, change_origin) VALUES (1, :origin) ON CONFLICT(singleton) DO UPDATE SET change_origin = excluded.change_origin;

-- name: read_position_update!
-- param: location: &str
-- param: progress: f32
-- param: content_hash: &str
UPDATE book SET read_pos = :location, read_progress = COALESCE(:progress, read_progress) WHERE content_hash = :content_hash;
