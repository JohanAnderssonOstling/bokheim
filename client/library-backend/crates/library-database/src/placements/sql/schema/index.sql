-- name: schema_placements_index &
CREATE UNIQUE INDEX IF NOT EXISTS idx_dir_live_path
ON dir(parent_id, name_key) WHERE deleted_at IS NULL AND id != '00000000-0000-0000-0000-000000000000';

-- Recursive folder traversal starts from parent_id but cannot use
-- idx_dir_live_path when SQLite cannot prove its extra root exclusion.
CREATE INDEX IF NOT EXISTS idx_dir_live_parent
ON dir(parent_id) WHERE deleted_at IS NULL;

-- local_hash lost its implicit PK index when it stopped being the key; the
-- scan-skip lookup queries by it on every file, so index it explicitly.
CREATE INDEX IF NOT EXISTS idx_book_dir_local_hash ON book_dir(local_hash);

-- The composite primary key starts with dir_id and cannot efficiently answer
-- the common "all placements for this book" lookup by itself.
CREATE INDEX IF NOT EXISTS idx_book_dir_book
ON book_dir(book_row_id, deleted_at, is_downloaded);

CREATE UNIQUE INDEX IF NOT EXISTS idx_book_dir_live_path
ON book_dir(dir_id, file_name) WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_book_dir_live_portable_name ON book_dir(dir_id,portable_name_key(file_name)) WHERE deleted_at IS NULL;
