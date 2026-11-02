-- Book identity is stable. Only the first uploaded byte revision is portable;
-- local revisions and historical aliases never enter the synchronization log.
CREATE TABLE IF NOT EXISTS book_content(
    book_id BLOB PRIMARY KEY REFERENCES book(content_hash) ON DELETE CASCADE,
    canonical_hash BLOB NOT NULL CHECK(length(canonical_hash)=32),
    size_bytes INTEGER CHECK(size_bytes>0)
) STRICT;
CREATE TABLE IF NOT EXISTS local_content_alias(
    byte_hash BLOB NOT NULL CHECK(length(byte_hash)=32),
    book_id BLOB NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
    PRIMARY KEY(byte_hash,book_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS local_file_content(
    book_id BLOB NOT NULL,
    dir_id TEXT NOT NULL,
    byte_hash BLOB NOT NULL CHECK(length(byte_hash)=32),
    PRIMARY KEY(book_id,dir_id),
    FOREIGN KEY(book_id) REFERENCES book(content_hash) ON DELETE CASCADE,
    FOREIGN KEY(dir_id) REFERENCES dir(id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS local_identity_scope(
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    scope BLOB NOT NULL CHECK(length(scope)=16)
) STRICT;

-- Unverified provenance is device-local and never freezes a server revision.
CREATE TABLE IF NOT EXISTS local_content_origin(
    book_id BLOB PRIMARY KEY REFERENCES book(content_hash) ON DELETE CASCADE,
    canonical_hash BLOB NOT NULL CHECK(length(canonical_hash)=32)
) STRICT;

-- Device-local evidence for unchanged external files. Never synchronized.
CREATE TABLE IF NOT EXISTS local_import_source(
    path TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    byte_hash BLOB NOT NULL CHECK(length(byte_hash)=32)
) STRICT, WITHOUT ROWID;
