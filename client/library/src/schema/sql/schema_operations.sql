-- name: attach_curated_taxonomy !
-- param: database: &str
ATTACH :database AS curated
/

-- name: mark_library_write_transaction &
UPDATE main.book SET row_id=row_id WHERE 0
/

-- name: curated_taxonomy_attached?
-- param: database_name: &str
SELECT EXISTS(SELECT 1 FROM pragma_database_list WHERE name=:database_name)
/

-- name: replica_id?
SELECT replica_id FROM sync_metadata WHERE singleton = 1
/

-- name: database_populated?
-- param: system_table_pattern: &str
SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name NOT LIKE :system_table_pattern)
/
