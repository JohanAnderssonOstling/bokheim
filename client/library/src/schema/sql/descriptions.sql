-- Canonical description storage, kept off the frequently read book row.
CREATE TABLE IF NOT EXISTS book_description(
    book_row_id INTEGER PRIMARY KEY REFERENCES book(row_id) ON DELETE CASCADE,
    description TEXT NOT NULL
);
