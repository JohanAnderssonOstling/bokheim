-- name: schema_annotations_tables &
-- Annotations.
create table if not exists annotation(
    id              TEXT primary key,
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    toc_ordinal     INTEGER,
    progress        REAL,
    body            BLOB not null,
    modified_at     INTEGER not null,
    deleted_at      INTEGER,
    check(toc_ordinal IS NULL OR toc_ordinal >= 0)
);

-- A small derived index of shared rules; ordinary annotations have no row.
create table if not exists annotation_recurrence(
    annotation_id TEXT primary key references annotation(id) on delete cascade,
    scope INTEGER not null check(scope between 1 and 3)
);
create index if not exists idx_annotation_recurrence_scope on annotation_recurrence(scope);
