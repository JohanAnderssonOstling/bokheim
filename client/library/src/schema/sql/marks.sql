-- Annotations.
create table if not exists annotation(
    id              TEXT primary key,
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    anchor_kind     TEXT not null check(anchor_kind in ('epub_cfi', 'pdf')),
    anchor_cfi      TEXT,
    anchor_page     INTEGER,
    anchor_rects    BLOB,
    anchor_fallback_cfi TEXT,
    exact_text      TEXT not null,
    style           TEXT not null check(style in ('highlight', 'underline', 'squiggly', 'strikethrough')),
    color           TEXT not null,
    note            TEXT not null default '',
    created_at      INTEGER not null,
    modified_at     INTEGER not null,
    deleted_at      INTEGER,
    check(
        (anchor_kind='epub_cfi' AND anchor_cfi IS NOT NULL AND anchor_page IS NULL AND anchor_rects IS NULL AND anchor_fallback_cfi IS NULL)
        OR
        (anchor_kind='pdf' AND anchor_cfi IS NULL AND anchor_page IS NOT NULL AND anchor_page >= 0 AND anchor_rects IS NOT NULL)
    )
);

create index if not exists idx_annotation_book on annotation(book_row_id, deleted_at, modified_at);

