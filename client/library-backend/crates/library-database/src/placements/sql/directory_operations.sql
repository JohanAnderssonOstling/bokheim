-- Directory traversal, canonical locations, placements and queued work.

-- name: directory_read_sync_state_version_body ?
-- param: state_key: &SqlParam<'_>
SELECT body FROM sync_state_version WHERE state_key=:state_key AND state_subkey='' AND state_kind IN ('directory_name','directory_parent','directory_lifecycle') AND body IS NOT NULL;

-- name: directory_live_parent_and_name ->
-- param: dir_id: &SqlParam<'_>
SELECT parent_id,name FROM dir WHERE id=:dir_id AND deleted_at IS NULL;

-- name: directory_read_dir_parent_id_name_2 ?
-- param: dir_id: &SqlParam<'_>
SELECT parent_id,name FROM dir WHERE id=:dir_id AND deleted_at IS NULL;

-- name: directory_work_roots ?
SELECT dir_id FROM local_directory_work;

-- name: directory_trashed_roots ?
-- param: root_ids: &SqlParam<'_>
SELECT id FROM dir WHERE id IN (SELECT value FROM json_each(:root_ids)) AND deleted_at IS NOT NULL;

-- name: directory_named_child ->
-- param: parent_id: &SqlParam<'_>
-- param: name: &SqlParam<'_>
SELECT id FROM dir WHERE parent_id=:parent_id AND name=:name AND deleted_at IS NULL AND id!=parent_id;

-- name: directory_any_parent ->
-- param: id: &SqlParam<'_>
SELECT parent_id FROM dir WHERE id=:id;

-- name: directory_projected_name ->
-- param: id: &SqlParam<'_>
SELECT name FROM dir WHERE id=:id;

-- name: directory_clear_work !
-- param: dir_id: &SqlParam<'_>
DELETE FROM local_directory_work WHERE dir_id=:dir_id;

-- name: directory_canonical_location ->
-- param: id: &SqlParam<'_>
SELECT COALESCE(canonical_parent_id,parent_id),COALESCE(canonical_name,name) FROM dir WHERE id=:id;

-- name: directory_live_book_exists ->
-- param: content_hash: &SqlParam<'_>
SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=:content_hash AND deleted_at IS NULL);

-- name: directory_live_subtree_placements ?
-- param: directory_ids: &SqlParam<'_>
SELECT bd.dir_id,b.content_hash FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.dir_id IN (SELECT value FROM json_each(:directory_ids)) AND bd.deleted_at IS NULL AND b.deleted_at IS NULL;

-- name: directory_placements_trashed_from ?
-- param: trash_origin_dir_id: &SqlParam<'_>
SELECT bd.dir_id,b.content_hash FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.trash_origin_dir_id=:trash_origin_dir_id;

-- name: directory_books_trashed_from ?
-- param: trash_origin_dir_id: &SqlParam<'_>
SELECT content_hash FROM book WHERE trash_origin_dir_id=:trash_origin_dir_id;

-- name: directory_unobserved_placements ?
-- param: content_hash: &SqlParam<'_>
SELECT bd.dir_id,bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash=:content_hash AND bd.deleted_at IS NULL AND NOT EXISTS (SELECT 1 FROM local_file_projection fp WHERE fp.content_hash=b.content_hash AND fp.dir_id=bd.dir_id);

-- name: directory_live_book_folders ?
-- param: book_row_id: &SqlParam<'_>
SELECT dir_id FROM book_dir WHERE book_row_id=:book_row_id AND deleted_at IS NULL;

-- name: directory_clear_placement_hash !
-- param: dir_id: &SqlParam<'_>
-- param: content_hash: &SqlParam<'_>
UPDATE book_dir SET local_hash='' WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: directory_clear_placement_download !
-- param: dir_id: &SqlParam<'_>
-- param: content_hash: &SqlParam<'_>
UPDATE book_dir SET is_downloaded=0 WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: directory_children?
-- param: parents: &str
SELECT id FROM dir WHERE parent_id IN (SELECT value FROM json_each(:parents)) AND id!=parent_id;

-- name: directory_trashed_children?
-- param: parents: &str
SELECT id FROM dir WHERE COALESCE(canonical_parent_id,parent_id) IN (SELECT value FROM json_each(:parents)) AND id!=COALESCE(canonical_parent_id,parent_id) AND deleted_at IS NOT NULL;

-- name: live_directory_location?
-- param: dir_id: &str
SELECT parent_id,name FROM dir WHERE id=:dir_id AND deleted_at IS NULL;
