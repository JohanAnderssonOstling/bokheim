-- Resolves local or synchronized book placements.

-- name: placements_upsert_dir!
-- param: id: &str
-- param: parent_id: &str
-- param: name: &str
INSERT INTO dir (id, parent_id, name, intent_parent_id, intent_name, intent_lifecycle, deleted_at)
VALUES (:id, :parent_id, :name, :parent_id, :name, 0, NULL)
ON CONFLICT(id) DO UPDATE SET
    parent_id = excluded.parent_id,
    name = excluded.name,
    intent_parent_id = excluded.parent_id,
    intent_name = excluded.name,
    intent_lifecycle = 0,
    deleted_at = NULL;

-- name: placements_live_directory_exists?
-- param: directory_id: &str
SELECT EXISTS(
    SELECT 1 FROM dir WHERE id=:directory_id AND deleted_at IS NULL
);

-- name: placements_live_file_name?
-- param: directory_id: &str
-- param: content_hash: &str
SELECT file_name FROM book_dir
WHERE dir_id = :directory_id
  AND book_row_id = (SELECT row_id FROM book WHERE content_hash = :content_hash)
  AND deleted_at IS NULL;

-- name: placements_folder_name_occupied?
-- param: parent_id: &str
-- param: name_key: &str
-- param: excluded_id: &str
SELECT EXISTS(SELECT 1 FROM dir WHERE parent_id=:parent_id AND name_key=:name_key AND deleted_at IS NULL AND id!='00000000-0000-0000-0000-000000000000' AND id!=:excluded_id);

-- name: placements_sibling_folder_names?
-- param: parent_id: &str
-- param: excluded_id: &str
SELECT name FROM dir WHERE parent_id=:parent_id AND id!=:excluded_id AND deleted_at IS NULL;

-- name: placements_file_name_occupied?
-- param: directory_id: &str
-- param: name_key: &str
SELECT EXISTS(SELECT 1 FROM book_dir WHERE dir_id=:directory_id AND portable_name_key(file_name)=:name_key AND deleted_at IS NULL);

-- name: placements_book_is_trashed?
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=:content_hash AND deleted_at IS NOT NULL);

-- name: placements_copy_book_baseline!
-- param: source_dir_id: &str
-- param: destination_dir_id: &str
-- param: content_hash: &str
UPDATE book_dir SET (checksum, size_bytes, checksum_seq) = (
    SELECT source.checksum, source.size_bytes, source.checksum_seq FROM book_dir source
    WHERE source.dir_id = :source_dir_id AND source.book_row_id = book_dir.book_row_id)
WHERE dir_id = :destination_dir_id AND book_row_id = (SELECT row_id FROM book WHERE content_hash = :content_hash)
  AND EXISTS(SELECT 1 FROM book_dir source
             WHERE source.dir_id = :source_dir_id AND source.book_row_id = book_dir.book_row_id AND source.checksum IS NOT NULL);

-- name: placements_live_folders?
SELECT id,parent_id,name FROM dir WHERE deleted_at IS NULL AND id!=parent_id;

-- name: copy_directory_record_insert!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
-- param: is_downloaded: i32
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,deleted_at,trash_origin_dir_id) VALUES(:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,'',:is_downloaded,NULL,NULL);

-- name: copy_directory_record_select?
-- param: dir_id: &str
SELECT name FROM dir WHERE id=:dir_id AND deleted_at IS NULL;

-- name: copy_directory_record_select_2?
-- param: parent_id: &str
SELECT name FROM dir WHERE parent_id=:parent_id AND deleted_at IS NULL;

-- name: copy_directory_record_select_3?
-- param: dir_id: &str
SELECT b.content_hash,bd.file_name,bd.is_downloaded FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.dir_id=:dir_id AND bd.deleted_at IS NULL AND b.deleted_at IS NULL;

-- name: copy_directory_record_select_4?
-- param: parent_id: &str
SELECT id,name FROM dir WHERE parent_id=:parent_id AND id!=parent_id AND deleted_at IS NULL;

-- name: create_directory_intent!
-- param: dir_id: &str
-- param: parent_id: &str
-- param: name: &str
-- param: projected_name: &str
-- Reserve a free display name until deterministic repair assigns the final
-- path. Publish the requested name, never this provisional display name.
INSERT INTO dir(id,parent_id,name,intent_parent_id,intent_name,intent_lifecycle,deleted_at)
VALUES(:dir_id,:parent_id,:projected_name,:parent_id,:name,0,NULL);

-- name: empty_trash_records_select?
SELECT content_hash FROM book WHERE deleted_at IS NOT NULL;

-- name: empty_trash_roots?
-- Folders the user put in Trash, as recorded by their synchronized lifecycle.
SELECT id FROM dir WHERE deleted_at IS NOT NULL AND intent_lifecycle=1
AND EXISTS(SELECT 1 FROM sync_state_version v WHERE v.state_kind='directory_lifecycle' AND v.state_key=dir.id AND v.state_subkey='' AND v.body IS NOT NULL);

-- name: library_trash_select?
SELECT content_hash,COALESCE(NULLIF(title,''),content_hash),format,deleted_at
         FROM book WHERE deleted_at IS NOT NULL
         ORDER BY deleted_at DESC,portable_name_key(COALESCE(title,content_hash)),content_hash;

-- name: library_trash_select_2?
SELECT id,name,deleted_at FROM dir WHERE deleted_at IS NOT NULL AND intent_lifecycle=1
AND EXISTS(SELECT 1 FROM sync_state_version v WHERE v.state_kind='directory_lifecycle' AND v.state_key=dir.id AND v.state_subkey='' AND v.body IS NOT NULL) ORDER BY deleted_at DESC,name_key,id;

-- name: move_directory_record_select?
-- param: dir_id: &str
SELECT parent_id,name FROM dir WHERE id=:dir_id AND deleted_at IS NULL;

-- name: purge_book_record_select?
-- param: content_hash: &str
SELECT row_id FROM book WHERE content_hash=:content_hash AND deleted_at IS NOT NULL;

-- name: purge_book_record_update!
-- param: book_row_id: i64
-- param: now: i64
UPDATE book_dir SET deleted_at=:now,trash_origin_dir_id=NULL WHERE book_row_id=:book_row_id AND deleted_at IS NULL;

-- name: purge_directories!
-- param: directories: &str
-- param: now: i64
UPDATE dir SET deleted_at=:now,intent_lifecycle=2 WHERE id IN (SELECT value FROM json_each(:directories)) AND deleted_at IS NOT NULL;

-- name: remove_book_placement_record_select?
-- param: dir_id: &str
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id WHERE bd.dir_id=:dir_id AND book.content_hash=:content_hash AND bd.deleted_at IS NULL AND book.deleted_at IS NULL);

-- name: remove_book_placement_record_select_2?
-- param: content_hash: &str
-- param: dir_id: &str
SELECT NOT EXISTS(SELECT 1 FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND dir_id!=:dir_id AND deleted_at IS NULL);

-- name: remove_book_placement_record_update!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: deleted_at: i64
UPDATE book_dir SET deleted_at=:deleted_at,is_downloaded=0,trash_origin_dir_id=NULL WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND deleted_at IS NULL;

-- name: restore_book_placement_record_insert!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,deleted_at,trash_origin_dir_id) VALUES(:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,'',0,NULL,NULL) ON CONFLICT(dir_id,book_row_id) DO UPDATE SET file_name=excluded.file_name,deleted_at=NULL,trash_origin_dir_id=NULL;

-- name: restore_book_placement_record_select?
-- param: dir_id: &str
-- param: content_hash: &str
SELECT file_name FROM book_dir WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) ORDER BY deleted_at IS NULL DESC, deleted_at DESC LIMIT 1;

-- name: restore_book_placement_record_select_2?
-- param: content_hash: &str
SELECT COALESCE(NULLIF(title,''),content_hash) FROM book WHERE content_hash=:content_hash;

-- name: restore_book_placement_record_select_3?
-- param: dir_id: &str
-- param: content_hash: &str
SELECT file_name FROM book_dir WHERE dir_id=:dir_id AND deleted_at IS NULL AND book_row_id!=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: restore_book_placement_record_update!
-- param: content_hash: &str
UPDATE book SET deleted_at=NULL,trash_origin_dir_id=NULL WHERE content_hash=:content_hash;

-- name: restore_book_record_with_parent_insert!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,last_scan) VALUES(:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,'',0,0) ON CONFLICT(dir_id,book_row_id) DO UPDATE SET deleted_at=NULL,trash_origin_dir_id=NULL,file_name=excluded.file_name,is_downloaded=0,last_scan=0,local_hash='';

-- name: restore_book_record_with_parent_select_2?
-- param: content_hash: &str
-- Trash preserves membership. Independent placement removals stay removed.
SELECT bd.dir_id,bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id
WHERE b.content_hash=:content_hash AND bd.deleted_at IS NULL ORDER BY bd.dir_id;

-- name: restore_book_record_with_parent_select_4?
-- param: dir_id: &str
-- Suppressed directories have a repaired display parent. Restore follows the
-- retained parent intent while visibility still comes from the projection.
SELECT COALESCE(intent_parent_id,parent_id),deleted_at IS NULL FROM dir WHERE id=:dir_id;

-- name: restore_book_fallback_name?
-- param: content_hash: &str
-- Reuse a local filename if available, without reviving its removed membership.
SELECT COALESCE((SELECT file_name FROM book_dir WHERE book_row_id=book.row_id ORDER BY dir_id LIMIT 1),content_hash)
FROM book WHERE content_hash=:content_hash;

-- name: restore_book_record_with_parent_update!
-- param: content_hash: &str
UPDATE book SET deleted_at=NULL,trash_origin_dir_id=NULL WHERE content_hash=:content_hash;

-- name: restore_directory_record_select_3?
-- param: trash_origin_dir_id: &str
SELECT b.content_hash,bd.dir_id,bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.trash_origin_dir_id=:trash_origin_dir_id OR (b.trash_origin_dir_id=:trash_origin_dir_id AND bd.deleted_at IS NULL);

-- name: restore_directory_record_update!
-- param: trash_origin_dir_id: &str
UPDATE book_dir SET deleted_at=NULL,trash_origin_dir_id=NULL WHERE trash_origin_dir_id=:trash_origin_dir_id;

-- name: restore_directory_record_update_2!
-- param: trash_origin_dir_id: &str
UPDATE book SET deleted_at=NULL,trash_origin_dir_id=NULL WHERE trash_origin_dir_id=:trash_origin_dir_id;

-- name: transfer_book_placement_record_insert!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
-- param: is_downloaded: i32
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,deleted_at,trash_origin_dir_id) VALUES(:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,'',:is_downloaded,NULL,NULL) ON CONFLICT(dir_id,book_row_id) DO UPDATE SET file_name=excluded.file_name,is_downloaded=excluded.is_downloaded,deleted_at=NULL,trash_origin_dir_id=NULL;

-- name: transfer_book_placement_record_select?
-- param: dir_id: &str
-- param: content_hash: &str
SELECT bd.file_name,bd.is_downloaded FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id WHERE bd.dir_id=:dir_id AND book.content_hash=:content_hash AND bd.deleted_at IS NULL AND book.deleted_at IS NULL;

-- name: trash_book_record_select?
-- param: content_hash: &str
SELECT bd.dir_id,bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash=:content_hash AND bd.deleted_at IS NULL AND b.deleted_at IS NULL;

-- name: trash_book_record_update!
-- param: content_hash: &str
-- param: deleted_at: i64
UPDATE book SET deleted_at=:deleted_at,trash_origin_dir_id=NULL WHERE content_hash=:content_hash AND deleted_at IS NULL;

-- name: trash_directory_record_update!
-- param: content_hash: &str
-- param: deleted_at: i64
-- param: trash_origin_dir_id: &str
UPDATE book SET deleted_at=:deleted_at,trash_origin_dir_id=:trash_origin_dir_id WHERE content_hash=:content_hash;

-- name: trash_directory_record_update_2!
-- param: dir_id: &str
-- param: deleted_at: i64
UPDATE dir SET deleted_at=:deleted_at,intent_lifecycle=1 WHERE id=:dir_id;

-- Trashing a folder: `subtree` is a JSON list of the folder and every folder
-- below it, computed once in Rust.

-- name: trash_subtree_placements?
-- param: subtree: &str
SELECT b.content_hash,bd.dir_id,bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id
WHERE bd.dir_id IN (SELECT value FROM json_each(:subtree)) AND bd.deleted_at IS NULL AND b.deleted_at IS NULL;

-- name: trash_subtree_only_books?
-- param: subtree: &str
-- Books with no live placement outside the subtree; trashing the folder trashes them.
SELECT DISTINCT book.content_hash FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id
WHERE bd.dir_id IN (SELECT value FROM json_each(:subtree)) AND bd.deleted_at IS NULL AND book.deleted_at IS NULL
  AND NOT EXISTS(SELECT 1 FROM book_dir outside WHERE outside.book_row_id=book.row_id AND outside.deleted_at IS NULL
                 AND outside.dir_id NOT IN (SELECT value FROM json_each(:subtree)));

-- name: trash_subtree_placements_delete!
-- param: subtree: &str
-- param: dir_id: &str
-- param: deleted_at: i64
-- The remaining placements of books that live on elsewhere.
UPDATE book_dir SET deleted_at=:deleted_at,is_downloaded=0,trash_origin_dir_id=:dir_id
WHERE dir_id IN (SELECT value FROM json_each(:subtree)) AND deleted_at IS NULL
  AND book_row_id IN (SELECT row_id FROM book WHERE deleted_at IS NULL);

-- name: placement_expects_hash?
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
SELECT EXISTS(SELECT 1 FROM book_dir JOIN book ON book.row_id = book_dir.book_row_id WHERE book_dir.dir_id = :dir_id AND book.content_hash = :content_hash AND book_dir.file_name = :file_name AND book_dir.deleted_at IS NULL AND book.deleted_at IS NULL);

-- name: add_book_placement!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
-- param: local_hash: &str
-- param: downloaded: bool
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,:local_hash,:downloaded);

-- name: seed_book!
-- param: content_hash: &str
-- param: title: Option<&str>
-- param: added_at: i64
-- param: format: i64
-- param: search_title: &str
INSERT INTO book(content_hash,title,added_at,format,search_title) VALUES(:content_hash,:title,:added_at,:format,:search_title);

-- name: seed_dir!
-- param: dir_id: &str
-- param: parent_id: &str
-- param: name: &str
INSERT INTO dir(id,parent_id,name,intent_parent_id,intent_name,intent_lifecycle) VALUES(:dir_id,:parent_id,:name,:parent_id,:name,0);

-- name: seed_shelf_dir!
-- param: dir_id: &str
INSERT INTO dir(id,parent_id,name,intent_parent_id,intent_name,intent_lifecycle) VALUES(:dir_id,'00000000-0000-0000-0000-000000000000','Shelf','00000000-0000-0000-0000-000000000000','Shelf',0);

-- name: mark_all_placements_deleted!
UPDATE book_dir SET deleted_at=1;

-- name: rename_dir!
-- param: name: &str
-- param: dir_id: &str
UPDATE dir SET name=:name,intent_name=:name WHERE id=:dir_id;

-- name: touch_all_placement_hashes!
UPDATE book_dir SET local_hash='changed-during-upload';

-- name: stale_all_placements!
UPDATE book_dir SET deleted_at=1, is_downloaded=0;

-- name: book_title?
-- param: content_hash: &str
SELECT title FROM book WHERE content_hash=:content_hash;

-- name: book_count?
-- param: content_hash: &str
SELECT count(*) FROM book WHERE content_hash=:content_hash;

-- name: book_count_with_title?
-- param: title: &str
SELECT count(*) FROM book WHERE title=:title;

-- name: count_books?
-- param: hashes_json: &str
SELECT count(*) FROM book WHERE content_hash IN (SELECT value FROM json_each(:hashes_json));

-- name: downloaded_placement_total?
SELECT SUM(is_downloaded) FROM book_dir;

-- name: dir_name?
-- param: dir_id: &str
SELECT name FROM dir WHERE id=:dir_id;

-- name: book_at_placement?
-- param: dir_id: &str
-- param: file_name: &str
SELECT book.content_hash FROM book_dir JOIN book ON book.row_id = book_dir.book_row_id
WHERE book_dir.dir_id = :dir_id AND book_dir.file_name = :file_name
  AND book_dir.deleted_at IS NULL AND book.deleted_at IS NULL LIMIT 1;
