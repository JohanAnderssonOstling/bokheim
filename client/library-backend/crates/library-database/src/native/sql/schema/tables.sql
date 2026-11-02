-- name: schema_native_tables &
-- Device-local work queues and ingestion output. None of this is replicated.
CREATE TABLE IF NOT EXISTS local_file_work(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        -- Soft trash/restore commands do not enqueue either operation.
        operation TEXT NOT NULL CHECK(operation IN ('remove','materialize')),
        content_hash BLOB NOT NULL,
        -- Materialization follows placement identity. Cleanup retains a physical snapshot.
        dir_id TEXT,
        relative_path TEXT,
        source_root TEXT,
        source_identity TEXT,
        source_library TEXT,
        CHECK((operation='materialize' AND dir_id IS NOT NULL AND relative_path IS NULL AND source_root IS NULL)
           OR (operation='remove' AND dir_id IS NULL AND relative_path IS NOT NULL)),
        UNIQUE(operation,content_hash,dir_id)
    );

CREATE UNIQUE INDEX IF NOT EXISTS idx_local_file_cleanup
ON local_file_work(content_hash,relative_path,COALESCE(source_root,''),COALESCE(source_identity,''),COALESCE(source_library,'')) WHERE operation='remove';

-- Staged before the registry switches roots; applied atomically on opening the destination.
CREATE TABLE IF NOT EXISTS local_storage_move(
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    source_root TEXT NOT NULL,
    source_identity TEXT,
    source_library TEXT,
    adopted_json TEXT NOT NULL DEFAULT '[]',
    destination_root TEXT NOT NULL,
    files_json TEXT NOT NULL,
    directories_json TEXT NOT NULL,
    applied INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS local_directory_work(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        dir_id TEXT NOT NULL UNIQUE
    );

CREATE TABLE IF NOT EXISTS local_book_work(id INTEGER PRIMARY KEY AUTOINCREMENT, content_hash BLOB NOT NULL UNIQUE);

CREATE TABLE IF NOT EXISTS local_purge_work(id INTEGER PRIMARY KEY AUTOINCREMENT, content_hash BLOB NOT NULL UNIQUE);

CREATE TABLE IF NOT EXISTS local_file_projection(
        content_hash BLOB NOT NULL, dir_id TEXT NOT NULL, relative_path TEXT NOT NULL,
        PRIMARY KEY(content_hash,dir_id)
    );

CREATE TABLE IF NOT EXISTS local_directory_projection(
        dir_id TEXT PRIMARY KEY,relative_path TEXT NOT NULL
    );

-- Device-local book state. Each writer updates only the fields it owns.
CREATE TABLE IF NOT EXISTS book_local_state(
        content_hash BLOB PRIMARY KEY NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
        inspection_format INTEGER,
        inspection_version INTEGER,
        -- Recording evidence extracted from an audiobook's files.
        recording_json TEXT,
        -- The navigation target resolved by the reader when saving position.
        reading_target TEXT,
        CHECK((inspection_format IS NULL) = (inspection_version IS NULL))
    ) STRICT;

CREATE TABLE IF NOT EXISTS pdf_reader_metadata (
        content_hash BLOB PRIMARY KEY NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
        metadata BLOB NOT NULL
    );

-- Device-local values for this library; initialized as one singleton row.
CREATE TABLE IF NOT EXISTS library_local_state(
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    scan_state TEXT NOT NULL DEFAULT 'idle' CHECK(scan_state IN ('idle','pending','failed')),
    taxonomy_revision TEXT,
    enrichment_policy TEXT
) STRICT;
