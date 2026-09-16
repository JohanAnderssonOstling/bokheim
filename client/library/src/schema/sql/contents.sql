-- Publication navigation, reading entry, and audiobook metadata.
-- The publication's own navigation document, stored so the library can draw a
-- book's contents without opening the file — which may be remote, and which the
-- reader would otherwise re-parse on every visit. One row per book, holding the
-- whole tree: it is always read whole and nothing joins against it, which is
-- why it is JSON rather than a row per entry. 'none' records that the file was
-- inspected and carries no navigation, so it is not parsed again.
create table if not exists book_toc(
    content_hash    TEXT primary key not null,
    entry_count     INTEGER not null check(entry_count >= 0),
    toc_source      TEXT not null check(toc_source in ('embedded_epub', 'pdf_outline', 'none')),
    toc_json        TEXT not null
);

-- Which navigation entry the reader was inside when it last saved a position.
-- Derived from the position and the book's own navigation, so it is local and
-- never synchronized: another device recomputes it when it opens the book.
-- Stored as the entry's target rather than its title, so a re-parsed navigation
-- document supplies the wording.
create table if not exists book_reading_entry(
    content_hash    TEXT primary key not null,
    target          TEXT not null
);

create table if not exists audiobook_metadata(
    content_hash    TEXT primary key not null,
    duration_ms     INTEGER not null check(duration_ms >= 0),
    chapter_count   INTEGER not null check(chapter_count >= 0),
    chapter_source  TEXT not null check(chapter_source in ('embedded_m4b', 'audnexus', 'full_audiobook_fallback')),
    chapters_json   TEXT not null
);
