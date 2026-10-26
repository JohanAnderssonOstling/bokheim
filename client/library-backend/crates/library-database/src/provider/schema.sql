CREATE TABLE IF NOT EXISTS library_storage (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
    configuration TEXT NOT NULL
) STRICT;

-- Canonical provider references projected from metadata sync. A confirmed
-- provider object is independent of Bokheim's server_has_book/server_has_cover.
CREATE TABLE IF NOT EXISTS provider_book (
    book_id BLOB PRIMARY KEY NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
    file TEXT NOT NULL,
    thumbnails TEXT,
    available INTEGER NOT NULL DEFAULT 1 CHECK(available IN (0,1))
) STRICT;

-- Device-only cache generation. A cover (including an inspected absence) is
-- valid only for the exact book revision and published thumbnail pair.
CREATE TABLE IF NOT EXISTS provider_cover_cache (
    book_id BLOB PRIMARY KEY NOT NULL REFERENCES provider_book(book_id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    has_cover INTEGER NOT NULL CHECK(has_cover IN (0,1))
) STRICT;

-- Device-local scan checkpoint and staged inventory. Interrupted enumeration
-- must not delete entries from the last complete inventory.
CREATE TABLE IF NOT EXISTS provider_scan (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
    generation TEXT NOT NULL,
    checkpoint TEXT NOT NULL,
    cursor TEXT,
    completed INTEGER NOT NULL DEFAULT 0 CHECK(completed IN (0, 1))
) STRICT;

CREATE TABLE IF NOT EXISTS provider_scan_entry (
    file_id TEXT PRIMARY KEY NOT NULL,
    generation TEXT NOT NULL,
    entry TEXT NOT NULL
) STRICT;

-- Device-owned imports. Neither upload capabilities nor unfinished additions
-- enter replica state. The complete provider response is persisted before
-- publishing the book's canonical metadata.
CREATE TABLE IF NOT EXISTS provider_upload (
    id TEXT PRIMARY KEY NOT NULL,
    draft TEXT NOT NULL,
    file_id TEXT,
    parent_id TEXT,
    session TEXT,
    confirmed_file TEXT
) STRICT;

CREATE TABLE IF NOT EXISTS provider_directory (
    folder_id TEXT PRIMARY KEY NOT NULL,
    directory_id TEXT NOT NULL
) STRICT;
