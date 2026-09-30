-- name: schema_assets_tables &
-- Device-local transfer and cover state, one row per book. None of it is
-- replicated. A book without a row has every value at its default. Writers
-- upsert from `book`, so a row exists only for a book this device knows.
CREATE TABLE IF NOT EXISTS book_transfer(
    content_hash TEXT PRIMARY KEY NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
    -- What the server holds: the book's bytes and its cover.
    server_has_book INTEGER NOT NULL DEFAULT 0,
    server_has_cover INTEGER NOT NULL DEFAULT 0,
    -- A requested download of the book's bytes, and when it was requested.
    book_download TEXT CHECK(book_download IN ('background', 'user_initiated')),
    book_download_at INTEGER,
    -- Local bytes waiting to be uploaded. `upload_id` changes whenever the
    -- upload is queued again; `upload_prepared` is the id whose local file
    -- work has finished, so the upload may start when the two match.
    upload_id INTEGER,
    upload_checksum TEXT,
    upload_size INTEGER CHECK(upload_size > 0),
    upload_prepared INTEGER,
    upload_rejected_reason TEXT,
    upload_rejected_at INTEGER,
    -- This device's cover. A cover is set once: made here from the book or
    -- its artwork, or downloaded, and never replaced.
    cover_state TEXT CHECK(cover_state IN ('pending', 'ready', 'no_cover')),
    cover_retry_after INTEGER NOT NULL DEFAULT 0,
    cover_rejected_at INTEGER
) STRICT;
