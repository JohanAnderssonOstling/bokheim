-- name: schema_native_index &
CREATE INDEX IF NOT EXISTS idx_local_file_projection_path ON local_file_projection(relative_path);
