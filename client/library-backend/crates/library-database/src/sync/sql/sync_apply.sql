-- Sync apply: remote change projection.



-- name: activate_remote_directory!
-- param: dir_id: &str
UPDATE dir SET deleted_at=NULL WHERE id=:dir_id;

-- name: apply_remote_book_dir_added!
-- param: dir_id: &str
-- param: content_hash: &BookId
-- param: file_name: &str
INSERT INTO book_dir (dir_id, book_row_id, file_name, local_hash, is_downloaded, deleted_at)
VALUES (
    :dir_id,
    (SELECT row_id FROM book WHERE content_hash=:content_hash),
    :file_name,
    '',
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
-- param: content_hash: &BookId
-- param: origin_folder_id: Option<&str>
INSERT INTO book_dir (dir_id, book_row_id, file_name, local_hash, is_downloaded, deleted_at, trash_origin_dir_id)
VALUES (
    :dir_id,
    (SELECT row_id FROM book WHERE content_hash=:content_hash),
    COALESCE((SELECT file_name FROM book_dir WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash)), 'Untitled'),
    '',
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

-- name: ensure_remote_directory_placeholder!
-- param: dir_id: &str
INSERT OR IGNORE INTO dir(id,parent_id,name,canonical_parent_id,canonical_name,lifecycle,deleted_at)
VALUES(:dir_id,'00000000-0000-0000-0000-000000000000','Pending','00000000-0000-0000-0000-000000000000','Pending',1,1);

-- name: hide_remote_directories!
UPDATE dir SET deleted_at=1 WHERE id != '00000000-0000-0000-0000-000000000000';

-- name: insert_book_language!
-- param: book_row_id: i64
-- param: language: &str
INSERT OR IGNORE INTO book_language(book_row_id,language) VALUES(:book_row_id,:language);

-- name: meta_update_book_metadata_state_preserving_ids_update_2!
-- param: content_hash: &BookId
-- param: title: &str
-- param: subtitle: Option<&str>
-- param: search_title: &str
UPDATE book SET title=:title,subtitle=:subtitle,search_title=:search_title
        WHERE content_hash=:content_hash AND (title IS NOT :title OR subtitle IS NOT :subtitle OR search_title IS NOT :search_title);

-- name: pdf_reader_metadata_upsert!
-- param: content_hash: &BookId
-- param: metadata: &[u8]
INSERT INTO pdf_reader_metadata VALUES(:content_hash,:metadata) ON CONFLICT(content_hash) DO UPDATE SET metadata=excluded.metadata WHERE metadata IS NOT excluded.metadata;

-- name: project_remote_directory!
-- param: dir_id: &str
-- param: parent_id: &str
-- param: name: &str
UPDATE dir SET parent_id=:parent_id, name=:name WHERE id=:dir_id;

-- name: queue_book_work_insert!
-- param: content_hash: &BookId
INSERT OR REPLACE INTO local_book_work(content_hash) VALUES(:content_hash);

-- name: read_sync_state_version_select?
-- param: state_kind: &str
-- param: state_key: &StateKey
-- param: state_subkey: &str
SELECT changed_at,conflict_rank,replica_id,replica_seq FROM sync_state_version
        WHERE state_kind=:state_kind AND state_key=:state_key AND state_subkey=:state_subkey;

-- name: set_remote_directory_lifecycle!
-- param: dir_id: &str
-- param: lifecycle: i32
UPDATE dir SET lifecycle=:lifecycle WHERE id=:dir_id;

-- name: set_remote_directory_name!
-- param: dir_id: &str
-- param: name: &str
UPDATE dir SET canonical_name=:name WHERE id=:dir_id;

-- name: set_remote_directory_parent!
-- param: dir_id: &str
-- param: parent_id: &str
UPDATE dir SET canonical_parent_id=:parent_id WHERE id=:dir_id;

-- name: reserve_write_lock!
UPDATE main.book SET row_id=row_id WHERE 0;

-- name: outbox_delete_for_ids!
-- param: payload: &str
DELETE FROM sync_outbox WHERE replica_seq IN (SELECT value FROM json_each(:payload));


-- name: pull_acknowledged_addresses ?
-- param: publisher_sequences: &SqlParam<'_>
SELECT state_kind,state_key,state_subkey FROM sync_outbox WHERE replica_seq IN (SELECT value FROM json_each(:publisher_sequences));

-- name: pull_book_exists ->
-- param: content_hash: &SqlParam<'_>
SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=:content_hash);

-- name: pull_ensure_book !
-- param: content_hash: &SqlParam<'_>
-- param: search_title: &SqlParam<'_>
INSERT OR IGNORE INTO book(content_hash,search_title) VALUES(:content_hash,:search_title);

-- name: pull_mark_book_dirty !
-- param: content_hash: &SqlParam<'_>
INSERT OR IGNORE INTO sync_projection_dirty VALUES('book_projection',:content_hash,'');
