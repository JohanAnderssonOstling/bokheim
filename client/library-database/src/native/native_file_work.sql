-- Native file reconciliation snapshots and acknowledgements.

-- name: native_directory_work_ids?
SELECT dir_id FROM local_directory_work ORDER BY id;

-- name: native_book_work_directory_ids?
SELECT DISTINCT bd.dir_id
FROM local_book_work work
JOIN book book ON book.content_hash = work.content_hash
JOIN book_dir bd ON bd.book_row_id = book.row_id AND bd.deleted_at IS NULL
ORDER BY bd.dir_id;

-- name: native_directory_snapshot?
-- param: dir_id: &str
SELECT paths.path, projection.relative_path, directory.parent_id, directory.name
FROM dir directory
JOIN dir_paths paths ON paths.id = directory.id
LEFT JOIN local_directory_projection projection ON projection.dir_id = directory.id
WHERE directory.id = :dir_id AND directory.deleted_at IS NULL;

-- name: native_directory_sibling_names?
-- param: parent_id: &str
SELECT name FROM dir WHERE parent_id = :parent_id AND deleted_at IS NULL;

-- name: native_directory_rename_conflict ->
-- param: dir_id: &str
-- param: name: &str
SELECT EXISTS(
    SELECT 1 FROM dir
    WHERE parent_id = (SELECT parent_id FROM dir WHERE id = :dir_id)
      AND id <> :dir_id AND name = :name AND deleted_at IS NULL
);

-- name: native_directory_rename!
-- param: dir_id: &str
-- param: name: &str
UPDATE dir SET name = :name WHERE id = :dir_id AND deleted_at IS NULL;

-- name: native_directory_queue_work!
-- param: dir_id: &str
INSERT INTO local_directory_work(dir_id) VALUES (:dir_id)
ON CONFLICT(dir_id) DO UPDATE SET id = excluded.id;

-- name: native_move_projection_paths!
-- param: prefix: &str
-- param: replacement: &str
-- param: upper: &str
UPDATE local_file_projection
SET relative_path = :replacement || substr(relative_path, length(:prefix) + 1)
WHERE relative_path >= :prefix AND relative_path < :upper;

-- name: native_set_directory_projection!
-- param: dir_id: &str
-- param: relative_path: &str
INSERT INTO local_directory_projection(dir_id, relative_path) VALUES (:dir_id, :relative_path)
ON CONFLICT(dir_id) DO UPDATE SET relative_path = excluded.relative_path;

-- name: requested_purge_books?
-- param: after: &str
SELECT content_hash
FROM local_purge_work
WHERE content_hash > :after
ORDER BY content_hash
LIMIT 32;

-- name: purge_has_pending_work ->
-- param: content_hash: &str
SELECT EXISTS(
    SELECT 1 FROM local_file_work WHERE content_hash = :content_hash
) OR EXISTS(
    SELECT 1 FROM local_book_work WHERE content_hash = :content_hash
) OR EXISTS(
    SELECT 1 FROM local_file_projection WHERE content_hash = :content_hash
);

-- name: purge_book_is_retired ->
-- param: content_hash: &str
SELECT EXISTS(
    SELECT 1 FROM book WHERE content_hash = :content_hash AND purged_at IS NOT NULL
);

-- name: acknowledge_purge_book!
-- param: content_hash: &str
DELETE FROM local_purge_work
WHERE content_hash = :content_hash;

-- Native book staging and conditional publish.



-- name: native_book_entry?
-- param: relative_path: &str
SELECT book.content_hash,bd.dir_id,bd.file_name FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id JOIN dir_paths paths ON paths.id=bd.dir_id
WHERE ('/' || CASE WHEN paths.path='' THEN bd.file_name ELSE paths.path || '/' || bd.file_name END)=:relative_path AND bd.deleted_at IS NULL LIMIT 1;

-- name: native_occupied_file_names?
-- param: dir_id: &str
SELECT file_name FROM book_dir WHERE dir_id=:dir_id AND deleted_at IS NULL;

-- name: native_book_name_conflict ->
-- param: dir_id: &str
-- param: name_key: &str
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.dir_id=:dir_id AND portable_name_key(bd.file_name)=:name_key AND b.content_hash!=:content_hash AND bd.deleted_at IS NULL);

-- name: native_rename_book!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: name: &str
UPDATE book_dir SET file_name=:name WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: native_publish_file_projection!
-- param: content_hash: &str
-- param: dir_id: &str
-- param: relative_path: &str
INSERT INTO local_file_projection(content_hash,dir_id,relative_path) VALUES(:content_hash,:dir_id,:relative_path) ON CONFLICT(content_hash,dir_id) DO UPDATE SET relative_path=excluded.relative_path;

-- name: native_set_book_fingerprint!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: fingerprint: &str
UPDATE book_dir SET local_hash=:fingerprint WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: native_purge_requested ->
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM local_purge_work WHERE content_hash=:content_hash);

-- name: native_file_work_rows?
SELECT id,operation,content_hash,relative_path FROM local_file_work ORDER BY id;

-- Lazy file-work resolution: current live placements for a hash, with
-- precomputed display paths. The queued path is only a hint; a directory
-- move after queueing must not strand the job.
-- name: native_live_placement_paths?
-- param: content_hash: &str
SELECT bd.dir_id, bd.file_name, ('/' || CASE WHEN paths.path='' THEN bd.file_name ELSE paths.path || '/' || bd.file_name END) FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id JOIN dir_paths paths ON paths.id=bd.dir_id WHERE book.content_hash=:content_hash AND bd.deleted_at IS NULL ORDER BY 3;

-- Last materialized locations for a hash; trash work for a retired book
-- resolves here instead of the stale queued path.
-- name: native_file_projection_paths?
-- param: content_hash: &str
SELECT dir_id, relative_path FROM local_file_projection WHERE content_hash=:content_hash ORDER BY relative_path;

-- name: native_file_precedes ->
-- param: content_hash: &str
-- param: id: i64
SELECT EXISTS(SELECT 1 FROM local_file_work WHERE content_hash=:content_hash AND id<:id);

-- name: native_file_is_current ->
-- param: id: i64
-- param: operation: &str
-- param: content_hash: &str
-- param: relative_path: &str
SELECT EXISTS(SELECT 1 FROM local_file_work WHERE id=:id AND operation=:operation AND content_hash=:content_hash AND relative_path=:relative_path);

-- name: native_book_is_live ->
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=:content_hash AND deleted_at IS NULL AND purged_at IS NULL);

-- name: native_current_checksum?
-- param: content_hash: &str
-- param: relative_path: &str
SELECT max(current.checksum) FROM local_book_current current JOIN book b ON b.content_hash=current.content_hash JOIN book_dir bd ON bd.book_row_id=b.row_id AND bd.dir_id=current.dir_id AND bd.deleted_at IS NULL JOIN dir_paths paths ON paths.id=bd.dir_id WHERE current.content_hash=:content_hash AND ('/' || CASE WHEN paths.path='' THEN bd.file_name ELSE paths.path||'/'||bd.file_name END)=:relative_path;

-- name: native_file_name_occupied ->
-- param: dir_id: &str
-- param: name: &str
SELECT EXISTS(SELECT 1 FROM book_dir WHERE dir_id=:dir_id AND file_name=:name AND deleted_at IS NULL);

-- name: native_delete_file_projection!
-- param: content_hash: &str
-- param: relative_path: &str
DELETE FROM local_file_projection WHERE content_hash=:content_hash AND relative_path=:relative_path;

-- name: native_mark_hash_downloaded!
-- param: content_hash: &str
UPDATE book_dir SET is_downloaded=1 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND deleted_at IS NULL;

-- name: native_complete_download!
-- param: content_hash: &str
DELETE FROM download_requests WHERE target_kind='book' AND target_key=:content_hash;

-- name: native_request_thumbnail!
-- param: content_hash: &str
INSERT INTO local_thumbnail_work(content_hash,state) VALUES(:content_hash,'pending') ON CONFLICT(content_hash) DO UPDATE SET state='pending',retry_after=0 WHERE local_thumbnail_work.state='ready';

-- name: native_delete_file_work!
-- param: id: i64
DELETE FROM local_file_work WHERE id=:id;

-- name: native_directory_book_path?
-- param: dir_id: &str
-- param: name: &str
SELECT '/' || CASE WHEN path='' THEN :name ELSE path || '/' || :name END FROM dir_paths WHERE id=:dir_id;

-- name: native_record_remote_book!
-- param: content_hash: &str
INSERT INTO remote_asset(kind,hash) VALUES('book',:content_hash) ON CONFLICT(kind,hash) DO NOTHING;

-- name: native_queue_book_work!
-- param: content_hash: &str
INSERT INTO local_book_work(content_hash) VALUES(:content_hash) ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;

-- name: native_has_pending_purge_work?
SELECT EXISTS(SELECT 1 FROM local_purge_work);

-- name: schema_jobs &
-- Device-local work queues and ingestion output. None of this is replicated.
CREATE TABLE IF NOT EXISTS local_file_work(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        operation TEXT NOT NULL CHECK(operation IN ('trash','restore')),
        content_hash TEXT NOT NULL REFERENCES book(content_hash),
        relative_path TEXT NOT NULL,
        UNIQUE(operation,content_hash,relative_path)
    );
    CREATE TABLE IF NOT EXISTS local_directory_work(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        dir_id TEXT NOT NULL UNIQUE
    );
    CREATE TABLE IF NOT EXISTS local_book_work(id INTEGER PRIMARY KEY AUTOINCREMENT, content_hash TEXT NOT NULL UNIQUE);
    CREATE TABLE IF NOT EXISTS local_purge_work(content_hash TEXT PRIMARY KEY NOT NULL REFERENCES book(content_hash));
    CREATE TABLE IF NOT EXISTS local_file_projection(
        content_hash TEXT NOT NULL, dir_id TEXT NOT NULL, relative_path TEXT NOT NULL,
        PRIMARY KEY(content_hash,dir_id)
    );
    CREATE INDEX IF NOT EXISTS idx_local_file_projection_path ON local_file_projection(relative_path);
    CREATE TABLE IF NOT EXISTS local_directory_projection(
        dir_id TEXT PRIMARY KEY,relative_path TEXT NOT NULL
    );

CREATE TABLE IF NOT EXISTS local_thumbnail_work(
        content_hash TEXT PRIMARY KEY REFERENCES book(content_hash) ON DELETE CASCADE,
        state TEXT NOT NULL CHECK(state IN ('pending','ready','no_cover')),
        retry_after INTEGER NOT NULL DEFAULT 0
    );

-- Byte provenance, separate from recovery state: ready does not mean authored here.
CREATE TABLE local_thumbnail_source(
    content_hash TEXT PRIMARY KEY NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
    origin TEXT NOT NULL CHECK(origin IN ('local','remote'))
) STRICT;

CREATE TABLE IF NOT EXISTS local_import_inspection(
        content_hash TEXT PRIMARY KEY REFERENCES book(content_hash) ON DELETE CASCADE,
        format TEXT NOT NULL,
        version INTEGER NOT NULL
    );

CREATE TABLE IF NOT EXISTS local_subject_pipeline(
        content_hash TEXT PRIMARY KEY REFERENCES book(content_hash) ON DELETE CASCADE,
        state TEXT NOT NULL, metadata BLOB NOT NULL, title TEXT NOT NULL,
        complete INTEGER NOT NULL DEFAULT 0, retry_at INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE IF NOT EXISTS local_isbn_response(
        isbn TEXT PRIMARY KEY, response TEXT NOT NULL, expires_at INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS local_isbn_response_expiry ON local_isbn_response(expires_at DESC,isbn);
    

CREATE TABLE IF NOT EXISTS local_upload_preparation (
    content_hash TEXT PRIMARY KEY REFERENCES book(content_hash) ON DELETE CASCADE,
    upload_id INTEGER
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
    CREATE INDEX IF NOT EXISTS audible_enrichment_due ON audible_enrichment_jobs(status,retry_at,lease_until);
    CREATE VIEW IF NOT EXISTS pending_audible_enrichment AS
        SELECT content_hash FROM audible_enrichment_jobs WHERE status IN ('pending','resolving');

CREATE TABLE IF NOT EXISTS pdf_reader_metadata (
        content_hash TEXT NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
        checksum TEXT NOT NULL, metadata BLOB NOT NULL, PRIMARY KEY(content_hash,checksum)
    );

CREATE TRIGGER IF NOT EXISTS pdf_reader_metadata_insert AFTER INSERT ON pdf_reader_metadata
BEGIN
    INSERT INTO sync_outbox(state_kind,state_key,state_subkey,body,changed_at,conflict_rank)
    SELECT 'pdf_reader_metadata',NEW.content_hash,NEW.checksum,bokheim_mutation('pdf_reader_metadata',NEW.content_hash,NEW.metadata),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    WHERE COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER IF NOT EXISTS pdf_reader_metadata_update AFTER UPDATE ON pdf_reader_metadata
BEGIN
    INSERT INTO sync_outbox(state_kind,state_key,state_subkey,body,changed_at,conflict_rank)
    SELECT 'pdf_reader_metadata',NEW.content_hash,NEW.checksum,bokheim_mutation('pdf_reader_metadata',NEW.content_hash,NEW.metadata),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    WHERE COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;

CREATE TRIGGER IF NOT EXISTS pdf_reader_metadata_purge AFTER UPDATE OF purged_at ON book
WHEN NEW.purged_at IS NOT NULL
BEGIN
    DELETE FROM pdf_reader_metadata WHERE content_hash=NEW.content_hash;
    DELETE FROM sync_outbox WHERE state_kind='pdf_reader_metadata' AND state_key=NEW.content_hash;
END;
