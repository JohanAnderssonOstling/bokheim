-- name: schema_enrichment_index &
create index if not exists idx_book_enrichment_retry on book_enrichment(retry_at);

