-- Catalog publication membership.
create table if not exists catalog_publication_book(
    source_id       TEXT not null,
    edition_id      TEXT not null,
    work_id         TEXT not null default '',
    book_row_id     INTEGER not null references book(row_id) on delete cascade,
    primary key(source_id, edition_id, work_id)
);

-- Remove the old work-level ownership index. Exact identity lookups use the
-- table's primary key.
