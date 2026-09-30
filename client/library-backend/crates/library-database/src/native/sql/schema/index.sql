-- name: schema_native_index &
CREATE INDEX IF NOT EXISTS idx_local_file_projection_path ON local_file_projection(relative_path);

CREATE INDEX IF NOT EXISTS local_isbn_response_expiry ON local_isbn_response(expires_at DESC,isbn);

CREATE INDEX IF NOT EXISTS audible_enrichment_due ON audible_enrichment_jobs(status,retry_at,lease_until);
