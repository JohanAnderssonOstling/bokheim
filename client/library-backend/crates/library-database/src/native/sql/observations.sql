-- Record local file observations, placement availability and purge work.

-- name: observations_mark_downloaded !
-- param: dir_id: &SqlParam<'_>
-- param: content_hash: &SqlParam<'_>
-- param: local_hash: &SqlParam<'_>
UPDATE book_dir SET is_downloaded=1,local_hash=COALESCE(:local_hash,'')
         WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND deleted_at IS NULL;

-- name: observations_mark_missing !
-- param: dir_id: &SqlParam<'_>
-- param: content_hash: &SqlParam<'_>
UPDATE book_dir SET is_downloaded=0,local_hash='',size_bytes=NULL
         WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: observations_remove_file !
-- param: dir_id: &SqlParam<'_>
-- param: content_hash: &SqlParam<'_>
DELETE FROM local_file_projection WHERE dir_id=:dir_id AND content_hash=:content_hash;

-- name: observations_file_directory ->
-- param: content_hash: &SqlParam<'_>
-- param: relative_path: &SqlParam<'_>
SELECT dir_id FROM local_file_projection WHERE content_hash=:content_hash AND relative_path=:relative_path;

-- name: observations_purge_work ->
-- param: content_hash: &SqlParam<'_>
SELECT id FROM local_purge_work WHERE content_hash=:content_hash;

-- name: observations_has_file_work ->
-- param: content_hash: &SqlParam<'_>
SELECT EXISTS(SELECT 1 FROM local_file_work WHERE content_hash=:content_hash);

-- name: observations_directory_path ->
-- param: dir_id: &SqlParam<'_>
SELECT '/' || ltrim(relative_path,'/') FROM local_directory_projection WHERE dir_id=:dir_id;
