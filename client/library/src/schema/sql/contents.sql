-- Book navigation, reading entry, and audiobook metadata.
-- The book's own navigation document, whatever the format called it:
-- a nav document, an outline, or a chapter list. Stored so the library can draw
-- a book's contents without opening the file — which may be remote, and which
-- the reader would otherwise re-parse on every visit. One row per book, holding
-- the whole tree: it is always read whole and nothing joins against its
-- entries, which is why it is JSON rather than a row per entry. An existing row
-- with no entries records that the file was inspected and carries no
-- navigation, so it is not parsed again.
--
-- Keyed by the book's identity, not by the bytes it was read from: an identity
-- survives a revision, and so do the targets, because a PDF's pagination and an
-- audiobook's offsets are both fixed by what the identity is derived from.
create table if not exists book_toc(
    content_hash    TEXT primary key not null,
    entry_count     INTEGER not null check(entry_count >= 0),
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

-- What an audiobook is, beyond its navigation. The chapters themselves live in
-- book_toc with every other format's; the duration stays here because it is not
-- navigation, and because it is what turns a chapter's start into its end.
create table if not exists audiobook_metadata(
    content_hash    TEXT primary key not null,
    duration_ms     INTEGER not null check(duration_ms >= 0)
);
