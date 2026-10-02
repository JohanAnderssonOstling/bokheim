-- name: schema_enrichment_tables &
-- The metadata server's last answer for a book's file facts. Local only:
-- projection overlays a found answer that matches the current facts.
create table if not exists book_enrichment(
    content_hash    TEXT primary key not null references book(content_hash) on delete cascade,
    request_key     TEXT not null,
    status          TEXT not null check(status in ('found', 'no_match', 'failed')),
    response        TEXT,
    retry_at        INTEGER not null,
    -- The answer's cover source was tried; a new answer tries again.
    cover_checked   INTEGER not null default 0 CHECK(cover_checked IN (0,1))
);
