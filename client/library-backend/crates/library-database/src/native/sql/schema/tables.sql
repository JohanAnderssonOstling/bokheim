-- name: schema_native_tables &
-- Device-local work queues and ingestion output. None of this is replicated.
CREATE TABLE IF NOT EXISTS local_file_work(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        operation TEXT NOT NULL CHECK(operation IN ('trash','restore')),
        content_hash TEXT NOT NULL,
        relative_path TEXT NOT NULL,
        UNIQUE(operation,content_hash,relative_path)
    );

CREATE TABLE IF NOT EXISTS local_directory_work(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        dir_id TEXT NOT NULL UNIQUE
    );

CREATE TABLE IF NOT EXISTS local_book_work(id INTEGER PRIMARY KEY AUTOINCREMENT, content_hash TEXT NOT NULL UNIQUE);

CREATE TABLE IF NOT EXISTS local_purge_work(id INTEGER PRIMARY KEY AUTOINCREMENT, content_hash TEXT NOT NULL UNIQUE);

CREATE TABLE IF NOT EXISTS local_file_projection(
        content_hash TEXT NOT NULL, dir_id TEXT NOT NULL, relative_path TEXT NOT NULL,
        PRIMARY KEY(content_hash,dir_id)
    );

CREATE TABLE IF NOT EXISTS local_directory_projection(
        dir_id TEXT PRIMARY KEY,relative_path TEXT NOT NULL
    );

CREATE TABLE IF NOT EXISTS local_import_inspection(
        content_hash TEXT PRIMARY KEY REFERENCES book(content_hash) ON DELETE CASCADE,
        format INTEGER NOT NULL,
        version INTEGER NOT NULL
    );

CREATE TABLE IF NOT EXISTS local_subject_pipeline(
        content_hash TEXT PRIMARY KEY REFERENCES book(content_hash) ON DELETE CASCADE,
        phase INTEGER NOT NULL, revision INTEGER NOT NULL,
        metadata BLOB NOT NULL, title TEXT NOT NULL,
        complete INTEGER NOT NULL DEFAULT 0, retry_at INTEGER NOT NULL DEFAULT 0
    );

CREATE TABLE IF NOT EXISTS local_isbn_response(
        isbn TEXT PRIMARY KEY, response TEXT NOT NULL, expires_at INTEGER NOT NULL
    );

CREATE TABLE IF NOT EXISTS audible_enrichment_jobs(
        content_hash TEXT PRIMARY KEY REFERENCES audiobook_metadata(content_hash) ON DELETE CASCADE,
        policy TEXT NOT NULL,
        request_json TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('pending','resolving','applied','no_match','unavailable','ambiguous')),
        response_json TEXT,
        claim_token TEXT,
        lease_until INTEGER,
        retry_at INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL
    );

CREATE TABLE IF NOT EXISTS pdf_reader_metadata (
        content_hash TEXT NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
        checksum TEXT NOT NULL, metadata BLOB NOT NULL, PRIMARY KEY(content_hash,checksum)
    );

CREATE TABLE IF NOT EXISTS local_scan_failure(path TEXT PRIMARY KEY, error TEXT NOT NULL);
