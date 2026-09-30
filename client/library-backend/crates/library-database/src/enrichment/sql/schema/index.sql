-- name: schema_enrichment_index &
create index if not exists idx_external_metadata_attempt_status on external_metadata_attempt(provider_id, status, book_row_id);

