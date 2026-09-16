-- Rebuildable browse projections. The rebuild statements that maintain
-- them live with the browse module; only their tables are created here.
--
-- One row in folder_book_ownership means a book has at least one live placement
-- in this folder's live subtree. Multiplicities preserve exact distinct-book and
-- downloaded counts when the same book is placed in several descendant folders.
CREATE TABLE IF NOT EXISTS folder_book_ownership(
    folder_id                    TEXT NOT NULL REFERENCES dir(id) ON DELETE CASCADE,
    book_row_id                  INTEGER NOT NULL REFERENCES book(row_id) ON DELETE CASCADE,
    placement_count              INTEGER NOT NULL CHECK(placement_count > 0),
    downloaded_placement_count   INTEGER NOT NULL CHECK(downloaded_placement_count >= 0 AND downloaded_placement_count <= placement_count),
    PRIMARY KEY(folder_id, book_row_id)
);

-- Foreign-key cascades and book-scoped diagnostics need the reverse lookup.
CREATE INDEX IF NOT EXISTS idx_folder_book_ownership_book
ON folder_book_ownership(book_row_id, folder_id);


-- Rebuildable browse dimensions keep normalization and hierarchy projection
-- out of interactive facet queries.
CREATE TABLE IF NOT EXISTS book_browse_format(
    book_row_id INTEGER PRIMARY KEY REFERENCES book(row_id) ON DELETE CASCADE,
    format_category INTEGER NOT NULL CHECK(format_category IN (1, 2, 4))
);

CREATE TABLE IF NOT EXISTS book_browse_language(
    book_row_id INTEGER NOT NULL REFERENCES book(row_id) ON DELETE CASCADE,
    base_language TEXT NOT NULL,
    PRIMARY KEY(book_row_id, base_language)
);
CREATE INDEX IF NOT EXISTS idx_book_browse_language_value ON book_browse_language(base_language, book_row_id);

-- A count is retained because several assigned concepts can place one book
-- under the same subject ancestor.
CREATE TABLE IF NOT EXISTS subject_book_ownership(
    subject_path TEXT NOT NULL,
    book_row_id INTEGER NOT NULL REFERENCES book(row_id) ON DELETE CASCADE,
    assignment_count INTEGER NOT NULL CHECK(assignment_count > 0),
    PRIMARY KEY(subject_path, book_row_id)
);
CREATE INDEX IF NOT EXISTS idx_subject_book_ownership_book ON subject_book_ownership(book_row_id, subject_path);

CREATE TABLE IF NOT EXISTS browse_projection_state(
    projection TEXT PRIMARY KEY,
    dirty INTEGER NOT NULL CHECK(dirty IN (0,1))
);
INSERT OR IGNORE INTO browse_projection_state(projection,dirty) VALUES('subject_ownership',0);
