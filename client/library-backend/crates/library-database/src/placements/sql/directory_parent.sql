-- name: live_directory_location?
-- param: dir_id: &str
SELECT parent_id,name FROM dir WHERE id=:dir_id AND deleted_at IS NULL;
