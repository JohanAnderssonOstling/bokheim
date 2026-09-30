-- name: schema_enrichment_tables &
-- External metadata attempts.
create table if not exists external_metadata_attempt(
    book_row_id     INTEGER not null references book(row_id) on delete cascade,
    provider_id     TEXT not null,
    identifier      TEXT not null,
    status          TEXT not null check(status in ('updated', 'no_match', 'ambiguous', 'failed')),
    detail          TEXT,
    attempted_at    INTEGER not null,
    primary key(book_row_id, provider_id)
);

-- Metadata coalescing triggers.
-- Version 4: description-independent metadata; connection-local batching mode; explicit upgrade definitions.
CREATE TABLE IF NOT EXISTS pending_metadata_payload(book_row_id INTEGER PRIMARY KEY REFERENCES book(row_id) ON DELETE CASCADE);
