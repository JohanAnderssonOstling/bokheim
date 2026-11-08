-- Device-local recording identity, independent of the transfer ZIP bytes.
-- Historical aliases retain stable book IDs when packaging changes.
CREATE TABLE IF NOT EXISTS local_audiobook_content(
    content_hash BLOB NOT NULL CHECK(length(content_hash)=32),
    book_id BLOB NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
    PRIMARY KEY(content_hash,book_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS local_audiobook_content_book ON local_audiobook_content(book_id);
