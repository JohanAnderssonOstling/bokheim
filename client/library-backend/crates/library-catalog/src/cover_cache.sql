CREATE TABLE IF NOT EXISTS cover(
    id INTEGER PRIMARY KEY,
    origin TEXT NOT NULL,
    book_key BLOB NOT NULL CHECK(length(book_key)=32),
    suffix TEXT NOT NULL,
    size INTEGER NOT NULL CHECK(size>=0),
    accessed INTEGER NOT NULL,
    UNIQUE(origin,book_key,suffix)
) STRICT;
CREATE INDEX IF NOT EXISTS cover_lru ON cover(accessed,id,size);
-- Recency updates must not rewrite a JPEG's overflow pages.
CREATE TABLE IF NOT EXISTS cover_data(
    id INTEGER PRIMARY KEY REFERENCES cover(id) ON DELETE CASCADE,
    bytes BLOB NOT NULL
) STRICT;
