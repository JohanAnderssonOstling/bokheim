-- Transfer presentation SQL migrated from library-backend.

-- Asset transfer planning and settlement.

-- name: transfer_ready_uploads?
-- param: hashes: &str
SELECT p.content_hash FROM json_each(:hashes) requested
JOIN local_upload_preparation p ON p.content_hash=requested.value
LEFT JOIN local_book_upload u ON u.content_hash=p.content_hash
WHERE u.id IS NULL OR p.upload_id=u.id;

-- name: transfer_local_versions?
-- param: content_hash: &str
SELECT dir_id, file_name, local_hash FROM book_dir JOIN book ON book.row_id=book_dir.book_row_id
WHERE book.content_hash=:content_hash AND book_dir.deleted_at IS NULL ORDER BY dir_id,file_name;

-- name: transfer_upload_intent?
-- param: content_hash: &str
SELECT id, checksum, size_bytes FROM local_book_upload WHERE content_hash=:content_hash;

-- Upload planning reads the intent queue directly instead of deriving
-- upload-ness from the asset_work drain. Same eligibility as the planning
-- flags: a live book with a live placement whose intent is not recently
-- rejected. Uploading needs local bytes, not path finality, so rootedness
-- is intentionally not required here.
-- name: transfer_pending_book_uploads?
-- param: limit: i64
SELECT upload.id, upload.content_hash, upload.checksum, upload.size_bytes FROM local_book_upload upload
WHERE EXISTS(SELECT 1 FROM book b WHERE b.content_hash=upload.content_hash AND b.deleted_at IS NULL)
AND EXISTS(SELECT 1 FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash=upload.content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL)
AND NOT EXISTS(SELECT 1 FROM rejected_asset_upload rejected WHERE rejected.kind='book' AND rejected.hash=upload.content_hash AND rejected.rejected_at>unixepoch()-300)
ORDER BY upload.id LIMIT :limit;

-- name: transfer_complete_upload!
-- param: id: i64
-- param: content_hash: &str
-- param: checksum: &str
DELETE FROM local_book_upload WHERE id=:id AND content_hash=:content_hash AND checksum=:checksum;

-- name: transfer_record_remote_book!
-- param: content_hash: &str
INSERT INTO remote_asset(kind,hash) VALUES ('book',:content_hash) ON CONFLICT(kind,hash) DO NOTHING;

-- name: transfer_forget_remote_book!
-- param: content_hash: &str
DELETE FROM remote_asset WHERE kind='book' AND hash=:content_hash;

-- name: transfer_is_book_downloaded?
-- param: content_hash: &str
SELECT COUNT(*) > 0
FROM book_dir
WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
  AND deleted_at is null
  AND is_downloaded = 1;

-- name: transfer_cancel_book_restore!
-- param: content_hash: &str
DELETE FROM local_file_work WHERE content_hash=:content_hash AND operation='restore';



-- name: transfer_has_pending_file_work?
SELECT EXISTS(SELECT 1 FROM local_directory_work)
    OR EXISTS(SELECT 1 FROM local_file_work)
    OR EXISTS(SELECT 1 FROM local_book_work);

-- name: transfer_set_book_hash_downloaded!
-- param: content_hash: &str
-- param: is_downloaded: i32
UPDATE book_dir
SET is_downloaded = :is_downloaded
WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
  AND deleted_at IS NULL;

-- name: transfer_record_remote_thumbnail!
-- param: content_hash: &str
INSERT INTO remote_asset(kind,hash) VALUES ('thumbnail',:content_hash) ON CONFLICT(kind,hash) DO NOTHING;

-- name: transfer_record_book_rejection!
-- param: content_hash: &str
-- param: reason: &str
INSERT INTO rejected_asset_upload(kind,hash,reason,rejected_at) VALUES ('book',:content_hash,:reason,unixepoch())
ON CONFLICT(kind,hash) DO UPDATE SET reason=excluded.reason,rejected_at=excluded.rejected_at;

-- name: transfer_record_thumbnail_rejection!
-- param: content_hash: &str
-- param: reason: &str
INSERT INTO rejected_asset_upload(kind,hash,reason,rejected_at) VALUES ('thumbnail',:content_hash,:reason,unixepoch())
ON CONFLICT(kind,hash) DO UPDATE SET reason=excluded.reason,rejected_at=excluded.rejected_at;

-- name: transfer_work_watermark ->
SELECT COALESCE(MAX(id),0) FROM asset_work;

-- Finding half of candidate planning: page identity and liveness only.
-- All allowance/state flags are checked separately in Rust over the page's
-- hashes, so each query below stays a plain existence check.
-- name: transfer_candidate_page?
-- param: after: i64
-- param: through: i64
SELECT w.id, w.content_hash, b.row_id, b.deleted_at FROM asset_work w
LEFT JOIN book b ON b.content_hash = w.content_hash
WHERE w.id > :after AND w.id <= :through ORDER BY w.id LIMIT 32;

-- Targeted drain entry: one asset_work row by hash for payload wakes.
-- Same shape as the page finder; shares the assemble path. Absent rows
-- (already settled) simply yield nothing.
-- name: transfer_candidate_row?
-- param: content_hash: &str
SELECT w.id, w.content_hash, b.row_id, b.deleted_at FROM asset_work w
LEFT JOIN book b ON b.content_hash = w.content_hash
WHERE w.content_hash = :content_hash;

-- Checking half of candidate planning: one allowance/state flag per hash,
-- each a plain existence check. No joins: every flag is answered by a single
-- indexed lookup, and directory work is intersected in Rust against the
-- walked ancestors instead.
-- name: transfer_candidate_flags?
-- param: hashes: &str
SELECT requested.value,
 NOT EXISTS(SELECT 1 FROM remote_asset remote WHERE remote.kind = 'thumbnail' AND remote.hash = requested.value)
   AND NOT EXISTS(SELECT 1 FROM rejected_asset_upload rejected WHERE rejected.kind = 'thumbnail' AND rejected.hash = requested.value AND rejected.rejected_at > unixepoch() - 300)
   AND EXISTS(SELECT 1 FROM local_thumbnail_work work WHERE work.content_hash = requested.value AND work.state = 'ready')
   AND EXISTS(SELECT 1 FROM local_thumbnail_source source WHERE source.content_hash = requested.value AND source.origin = 'local'),
 EXISTS(SELECT 1 FROM remote_asset remote WHERE remote.kind = 'thumbnail' AND remote.hash = requested.value),
 EXISTS(SELECT 1 FROM local_thumbnail_work work WHERE work.content_hash = requested.value AND work.state = 'pending'),
 EXISTS(SELECT 1 FROM download_requests request WHERE request.target_kind = 'book' AND request.target_key = requested.value),
 EXISTS(SELECT 1 FROM rejected_asset_upload rejected WHERE (rejected.kind = 'book' OR rejected.kind = 'thumbnail') AND rejected.hash = requested.value AND rejected.rejected_at > unixepoch() - 300),
 EXISTS(SELECT 1 FROM local_book_work work WHERE work.content_hash = requested.value),
 EXISTS(SELECT 1 FROM local_file_work work WHERE work.operation IN ('restore', 'trash') AND work.content_hash = requested.value)
FROM json_each(:hashes) requested;

-- name: transfer_settle_candidates!
-- param: candidates: &str
WITH RECURSIVE candidates AS MATERIALIZED (SELECT w.id,w.content_hash,json_extract(j.value,'$[1]') AS check_file_work FROM json_each(:candidates) j JOIN asset_work w ON w.id=json_extract(j.value,'$[0]')), ancestors(work_id,id) AS MATERIALIZED (
 SELECT c.id,bd.dir_id FROM candidates c JOIN book b ON b.content_hash=c.content_hash JOIN book_dir bd ON bd.book_row_id=b.row_id
 UNION SELECT c.id,f.dir_id FROM candidates c JOIN local_file_projection f ON f.content_hash=c.content_hash
 UNION SELECT a.work_id,d.parent_id FROM ancestors a JOIN dir d ON d.id=a.id WHERE d.id!=d.parent_id)
DELETE FROM asset_work WHERE id IN (SELECT c.id FROM candidates c WHERE NOT c.check_file_work OR (NOT EXISTS(SELECT 1 FROM local_book_work WHERE content_hash=c.content_hash) AND NOT EXISTS(SELECT 1 FROM local_file_work WHERE operation IN ('restore','trash') AND content_hash=c.content_hash) AND NOT EXISTS(SELECT 1 FROM ancestors a JOIN local_directory_work w ON w.dir_id=a.id WHERE a.work_id=c.id)));

-- Book paths are walked in Rust (see placement_paths); this query file no
-- longer needs its own copy of the directory recursion. The walk below
-- needs only single-row lookups, one per step.

-- name: transfer_dir_entry?
-- param: id: &str
SELECT parent_id, name, deleted_at FROM dir WHERE id = :id;

-- name: transfer_book_dirs?
-- param: book_row_id: i64
SELECT dir_id, file_name, deleted_at FROM book_dir WHERE book_row_id = :book_row_id;

-- name: transfer_projection_dirs?
-- param: content_hash: &str
SELECT dir_id FROM local_file_projection WHERE content_hash = :content_hash;

-- name: transfer_complete_download!
-- param: content_hash: &str
DELETE FROM download_requests WHERE target_kind='book' AND target_key=:content_hash;

-- name: transfer_live_placements?
-- param: content_hash: &str
SELECT bd.dir_id,bd.file_name,'/' || CASE WHEN paths.path='' THEN bd.file_name ELSE paths.path || '/' || bd.file_name END FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id JOIN dir_paths paths ON paths.id=bd.dir_id WHERE b.content_hash=:content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL;

-- name: transfer_queue_restore!
-- param: content_hash: &str
-- param: relative_path: &str
INSERT OR IGNORE INTO local_file_work(operation,content_hash,relative_path) VALUES ('restore',:content_hash,:relative_path);

-- name: transfer_placement_matches ->
-- param: content_hash: &str
-- param: relative_path: &str
SELECT EXISTS(SELECT 1 FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id JOIN dir_paths paths ON paths.id=bd.dir_id WHERE b.content_hash=:content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL AND ('/' || CASE WHEN paths.path='' THEN bd.file_name ELSE paths.path || '/' || bd.file_name END)=:relative_path);

-- name: transfer_placement_directory?
-- param: content_hash: &str
-- param: relative_path: &str
SELECT bd.dir_id FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id JOIN dir_paths paths ON paths.id=bd.dir_id WHERE b.content_hash=:content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL AND ('/' || CASE WHEN paths.path='' THEN bd.file_name ELSE paths.path || '/' || bd.file_name END)=:relative_path;

-- name: transfer_download_placement?
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book b JOIN book_dir bd ON bd.book_row_id=b.row_id WHERE b.content_hash=:content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL);

-- name: transfer_add_download_request!
-- param: target_key: &str
-- param: origin: &str
-- param: created_at: i64
INSERT INTO download_requests(target_kind, target_key, origin, created_at)
VALUES ('book', :target_key, :origin, :created_at)
ON CONFLICT(target_kind, target_key) DO UPDATE SET
    origin = CASE WHEN excluded.origin = 'user_initiated' THEN 'user_initiated' ELSE download_requests.origin END;

-- Download planning reads the request queue directly instead of deriving
-- from the asset_work drain. FIFO by insertion; user-facing work runs
-- ahead of bulk uploads.
-- name: transfer_pending_download_requests?
SELECT target_key FROM download_requests WHERE target_kind='book' ORDER BY rowid;

-- Durable download state in one row: bytes marked downloaded anywhere, and
-- a live placement with a queued request. Single-row existence checks only.
-- name: transfer_download_state?
-- param: content_hash: &str
SELECT
    EXISTS (
        SELECT 1
        FROM book_dir downloaded
        WHERE downloaded.book_row_id = (SELECT row_id FROM book WHERE content_hash = :content_hash)
          AND downloaded.deleted_at IS NULL
          AND downloaded.is_downloaded = 1
    ),
    EXISTS (
        SELECT 1
        FROM book_dir placement
        JOIN book ON book.row_id = placement.book_row_id
        JOIN download_requests request
          ON request.target_kind = 'book'
         AND request.target_key = book.content_hash
        WHERE book.content_hash = :content_hash
          AND placement.deleted_at IS NULL
    );

-- name: transfer_uploads_waiting_for_storage ->
SELECT EXISTS (
    SELECT 1 FROM rejected_asset_upload rejected
    JOIN local_book_upload upload ON upload.content_hash = rejected.hash
    WHERE rejected.kind = 'book'
      AND rejected.reason = 'server rejected upload admission: QuotaExceeded'
      AND rejected.rejected_at > unixepoch() - 300
);

-- name: transfer_first_file_names?
-- param: hashes: &str
SELECT requested.value, (SELECT bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash=requested.value ORDER BY bd.file_name LIMIT 1)
FROM json_each(:hashes) requested;

-- name: transfer_directory_work?
SELECT dir_id FROM local_directory_work;

-- name: transfer_book_upload_rejection_reason?
-- param: content_hash: &str
SELECT max(rejected.reason) FROM rejected_asset_upload rejected
JOIN local_book_upload upload ON upload.content_hash=rejected.hash
WHERE rejected.kind='book' AND rejected.hash=:content_hash AND rejected.rejected_at>unixepoch()-300

-- name: transfer_has_pending_asset_work?
WITH RECURSIVE live_dirs(id) AS (
            SELECT id FROM dir WHERE id='00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
            UNION ALL
            SELECT d.id FROM dir d JOIN live_dirs p ON d.parent_id=p.id AND d.id!=p.id WHERE d.deleted_at IS NULL
        ), live_books AS (
            SELECT b.content_hash,
                EXISTS(SELECT 1 FROM book_dir bd JOIN live_dirs d ON d.id=bd.dir_id
                    WHERE bd.book_row_id=b.row_id AND bd.deleted_at IS NULL AND bd.is_downloaded=1) AS local
            FROM book b WHERE b.content_hash!='' AND EXISTS(
                SELECT 1 FROM book_dir bd JOIN live_dirs d ON d.id=bd.dir_id
                WHERE bd.book_row_id=b.row_id AND bd.deleted_at IS NULL)
        )
        SELECT EXISTS(SELECT 1 FROM download_requests WHERE target_kind='book' AND origin='user_initiated')
        OR EXISTS(
            SELECT 1 FROM live_books b
            LEFT JOIN local_book_upload upload ON upload.content_hash=b.content_hash
            LEFT JOIN remote_asset rt ON rt.kind='thumbnail' AND rt.hash=b.content_hash
            LEFT JOIN rejected_asset_upload xb ON xb.kind='book' AND xb.hash=b.content_hash AND xb.rejected_at>unixepoch()-300
            LEFT JOIN rejected_asset_upload xt ON xt.kind='thumbnail' AND xt.hash=b.content_hash AND xt.rejected_at>unixepoch()-300
            LEFT JOIN local_thumbnail_work t ON t.content_hash=b.content_hash
            WHERE (b.local AND upload.id IS NOT NULL AND xb.hash IS NULL)
                OR (t.state='ready' AND rt.hash IS NULL AND xt.hash IS NULL AND EXISTS(SELECT 1 FROM local_thumbnail_source source WHERE source.content_hash=b.content_hash AND source.origin='local'))
                OR (COALESCE(t.state NOT IN ('ready','pending'),1) AND (NOT b.local OR rt.hash IS NOT NULL))
        )

-- name: schema_assets &
-- Remote asset presence, download requests, transfer work, upload intent.
create table if not exists remote_asset(
    kind    TEXT not null check(kind in ('book', 'thumbnail')),
    hash    TEXT not null,
    primary key (kind, hash)
);

-- Durable download intent for content-addressed assets.
create table if not exists download_requests(
    target_kind     TEXT not null check(target_kind in ('book', 'thumbnail')),
    target_key      TEXT not null,
    origin          TEXT not null default 'background' check(origin in ('background', 'user_initiated')),
    created_at      INTEGER not null,
    primary key (target_kind, target_key)
);


CREATE TABLE asset_work(id INTEGER PRIMARY KEY AUTOINCREMENT, content_hash TEXT NOT NULL UNIQUE);

CREATE TABLE IF NOT EXISTS local_book_versions(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        content_hash TEXT NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
        checksum TEXT NOT NULL,
        size_bytes INTEGER NOT NULL CHECK(size_bytes>0),
        origin TEXT NOT NULL CHECK(origin IN ('local','remote')),
        UNIQUE(content_hash,checksum)
    );
    CREATE TABLE IF NOT EXISTS local_book_upload(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        content_hash TEXT NOT NULL UNIQUE REFERENCES book(content_hash) ON DELETE CASCADE,
        checksum TEXT NOT NULL,
        size_bytes INTEGER NOT NULL CHECK(size_bytes>0)
    );
    CREATE TABLE IF NOT EXISTS local_book_current(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        dir_id TEXT NOT NULL,
        content_hash TEXT NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
        checksum TEXT NOT NULL,
        origin TEXT NOT NULL CHECK(origin IN ('local','remote')),
        UNIQUE(dir_id,content_hash)
    );

CREATE TABLE IF NOT EXISTS rejected_asset_upload(
            kind TEXT NOT NULL CHECK(kind IN ('book','thumbnail')),
            hash TEXT NOT NULL,
            reason TEXT NOT NULL,
            rejected_at INTEGER NOT NULL,
            PRIMARY KEY(kind, hash)
        );

-- name: schema_asset_triggers &
-- Triggers maintaining durable transfer work.
-- Durable asset candidates, independent of metadata exchange scheduling.
CREATE TRIGGER asset_work_book_insert AFTER INSERT ON book
BEGIN
    INSERT INTO asset_work(content_hash) SELECT NEW.content_hash WHERE NEW.content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_book_delete AFTER DELETE ON book
BEGIN
    INSERT INTO asset_work(content_hash) SELECT OLD.content_hash WHERE OLD.content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_book_dir_insert AFTER INSERT ON book_dir
BEGIN
    INSERT INTO asset_work(content_hash) SELECT content_hash FROM book WHERE row_id=NEW.book_row_id AND content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_book_dir_update AFTER UPDATE ON book_dir
BEGIN
    INSERT INTO asset_work(content_hash) SELECT content_hash FROM book WHERE row_id=NEW.book_row_id AND content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_book_dir_delete AFTER DELETE ON book_dir
BEGIN
    INSERT INTO asset_work(content_hash) SELECT content_hash FROM book WHERE row_id=OLD.book_row_id AND content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_remote_asset_insert AFTER INSERT ON remote_asset
BEGIN
    INSERT INTO asset_work(content_hash) SELECT NEW.hash WHERE NEW.hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_remote_asset_update AFTER UPDATE ON remote_asset
BEGIN
    INSERT INTO asset_work(content_hash) SELECT NEW.hash WHERE NEW.hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_remote_asset_delete AFTER DELETE ON remote_asset
BEGIN
    INSERT INTO asset_work(content_hash) SELECT OLD.hash WHERE OLD.hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_local_thumbnail_work_insert AFTER INSERT ON local_thumbnail_work
BEGIN
    INSERT INTO asset_work(content_hash) SELECT NEW.content_hash WHERE NEW.content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_local_thumbnail_work_update AFTER UPDATE ON local_thumbnail_work
BEGIN
    INSERT INTO asset_work(content_hash) SELECT NEW.content_hash WHERE NEW.content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_local_thumbnail_work_delete AFTER DELETE ON local_thumbnail_work
BEGIN
    INSERT INTO asset_work(content_hash) SELECT OLD.content_hash WHERE OLD.content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_download_requests_insert AFTER INSERT ON download_requests
BEGIN
    INSERT INTO asset_work(content_hash) SELECT NEW.target_key WHERE NEW.target_kind='book'
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_download_requests_update AFTER UPDATE ON download_requests
BEGIN
    INSERT INTO asset_work(content_hash) SELECT NEW.target_key WHERE NEW.target_kind='book'
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_download_requests_delete AFTER DELETE ON download_requests
BEGIN
    INSERT INTO asset_work(content_hash) SELECT OLD.target_key WHERE OLD.target_kind='book'
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_rejected_asset_upload_insert AFTER INSERT ON rejected_asset_upload
BEGIN
    INSERT INTO asset_work(content_hash) SELECT NEW.hash WHERE NEW.hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_rejected_asset_upload_update AFTER UPDATE ON rejected_asset_upload
BEGIN
    INSERT INTO asset_work(content_hash) SELECT NEW.hash WHERE NEW.hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_rejected_asset_upload_delete AFTER DELETE ON rejected_asset_upload
BEGIN
    INSERT INTO asset_work(content_hash) SELECT OLD.hash WHERE OLD.hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_directory_update AFTER UPDATE OF parent_id,name,deleted_at ON dir
BEGIN
    INSERT INTO asset_work(content_hash) WITH RECURSIVE affected(id) AS (
        SELECT NEW.id UNION ALL SELECT d.id FROM dir d JOIN affected a ON d.parent_id=a.id WHERE d.id!=d.parent_id
    ) SELECT DISTINCT b.content_hash FROM affected a JOIN book_dir bd ON bd.dir_id=a.id JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;
CREATE TRIGGER asset_work_directory_delete AFTER DELETE ON dir
BEGIN
    INSERT INTO asset_work(content_hash) WITH RECURSIVE affected(id) AS (
        SELECT OLD.id UNION ALL SELECT d.id FROM dir d JOIN affected a ON d.parent_id=a.id WHERE d.id!=d.parent_id
    ) SELECT DISTINCT b.content_hash FROM affected a JOIN book_dir bd ON bd.dir_id=a.id JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;

CREATE TRIGGER asset_work_directory_insert AFTER INSERT ON dir
BEGIN
    INSERT INTO asset_work(content_hash) WITH RECURSIVE affected(id) AS (
        SELECT NEW.id UNION ALL SELECT d.id FROM dir d JOIN affected a ON d.parent_id=a.id WHERE d.id!=d.parent_id
    ) SELECT DISTINCT b.content_hash FROM affected a JOIN book_dir bd ON bd.dir_id=a.id JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;

CREATE TRIGGER asset_work_old_placement AFTER UPDATE OF book_row_id ON book_dir
WHEN OLD.book_row_id!=NEW.book_row_id
BEGIN
    INSERT INTO asset_work(content_hash) SELECT content_hash FROM book WHERE row_id=OLD.book_row_id AND content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;

CREATE TRIGGER asset_work_book_update AFTER UPDATE OF content_hash,format,deleted_at,purged_at,hidden_at ON book
WHEN OLD.content_hash IS NOT NEW.content_hash OR OLD.format IS NOT NEW.format
  OR OLD.deleted_at IS NOT NEW.deleted_at OR OLD.purged_at IS NOT NEW.purged_at
  OR OLD.hidden_at IS NOT NEW.hidden_at
BEGIN
    INSERT INTO asset_work(content_hash) SELECT NEW.content_hash WHERE NEW.content_hash!=''
    ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
END;

-- name: book_row_id_select?
-- param: content_hash: &str
SELECT row_id FROM book WHERE content_hash = :content_hash;

-- name: record_remote_asset!
-- param: kind: &str
-- param: content_hash: &str
INSERT INTO remote_asset(kind, hash) VALUES (:kind, :content_hash) ON CONFLICT(kind, hash) DO NOTHING;

-- name: forget_remote_asset!
-- param: kind: &str
-- param: content_hash: &str
DELETE FROM remote_asset WHERE kind = :kind AND hash = :content_hash;

-- name: remote_asset_hashes?
-- param: kind: &str
SELECT hash FROM remote_asset WHERE kind = :kind;

-- name: add_asset_request!
-- param: kind: &str
-- param: content_hash: &str
-- param: origin: &str
-- param: created_at: i64
INSERT INTO download_requests(target_kind, target_key, origin, created_at) VALUES (:kind, :content_hash, :origin, :created_at)
ON CONFLICT(target_kind, target_key) DO UPDATE SET
    origin = CASE WHEN excluded.origin = 'user_initiated' THEN 'user_initiated' ELSE download_requests.origin END;

-- name: asset_requests?
-- param: kind: &str
SELECT target_key, origin FROM download_requests WHERE target_kind = :kind ORDER BY created_at, target_key;

-- name: rejected_uploads_prune!
-- param: cutoff: i64
DELETE FROM rejected_asset_upload WHERE rejected_at <= :cutoff;

-- name: rejected_upload_hashes?
-- param: kind: &str
-- param: cutoff: i64
SELECT hash FROM rejected_asset_upload WHERE kind = :kind AND rejected_at > :cutoff;

-- name: download_requested?
-- param: content_hash: &str
SELECT EXISTS (
        SELECT 1
        FROM book_dir placement
        JOIN book ON book.row_id = placement.book_row_id
        JOIN download_requests request
          ON request.target_kind = 'book'
         AND request.target_key = book.content_hash
        WHERE book.content_hash = :content_hash
          AND placement.deleted_at IS NULL
    );

-- name: observe_local_book_insert!
-- param: content_hash: &str
-- param: checksum: &str
-- param: size_bytes: i64
-- param: origin: &str
INSERT INTO local_book_versions(content_hash,checksum,size_bytes,origin) VALUES(:content_hash,:checksum,:size_bytes,:origin) ON CONFLICT(content_hash,checksum) DO NOTHING;

-- name: current_checksum_select?
-- param: dir_id: &str
-- param: content_hash: &str
SELECT checksum FROM local_book_current WHERE dir_id=:dir_id AND content_hash=:content_hash;

-- name: version_origin_select?
-- param: content_hash: &str
-- param: checksum: &str
SELECT origin FROM local_book_versions WHERE content_hash=:content_hash AND checksum=:checksum;

-- name: current_replace!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: checksum: &str
-- param: origin: &str
INSERT INTO local_book_current(dir_id,content_hash,checksum,origin) VALUES(:dir_id,:content_hash,:checksum,:origin)
        ON CONFLICT(dir_id,content_hash) DO UPDATE SET id=excluded.id,checksum=excluded.checksum,origin=excluded.origin;

-- name: enqueue_upload_insert!
-- param: content_hash: &str
-- param: checksum: &str
-- param: size_bytes: i64
INSERT INTO local_book_upload(content_hash,checksum,size_bytes) VALUES(:content_hash,:checksum,:size_bytes)
        ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id,checksum=excluded.checksum,size_bytes=excluded.size_bytes;

-- name: rejected_book_cleanup!
-- param: content_hash: &str
DELETE FROM rejected_asset_upload WHERE kind='book' AND hash=:content_hash;

-- name: asset_work_touch!
-- param: content_hash: &str
INSERT INTO asset_work(content_hash) VALUES(:content_hash) ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;

-- name: asset_work_page?
-- param: after: i64
-- param: through: i64
SELECT id,content_hash FROM asset_work WHERE id>:after AND id<=:through ORDER BY id LIMIT 32;

-- name: pipeline_members?
SELECT b.content_hash, u.id FROM book b LEFT JOIN local_book_upload u ON u.content_hash=b.content_hash WHERE b.deleted_at IS NULL ORDER BY b.content_hash;

-- name: pipeline_prepare!
-- param: members_json: &str
-- param: policy: &str
INSERT INTO local_upload_preparation(content_hash,upload_id)
SELECT b.content_hash, json_extract(member.value,'$[1]')
FROM json_each(:members_json) member
JOIN book b ON b.content_hash=json_extract(member.value,'$[0]')
WHERE b.deleted_at IS NULL
AND NOT EXISTS(SELECT 1 FROM audible_enrichment_jobs j
    WHERE j.content_hash=b.content_hash AND j.policy=:policy AND j.status IN ('pending','resolving'))
ON CONFLICT(content_hash) DO UPDATE SET upload_id=excluded.upload_id
WHERE local_upload_preparation.upload_id IS NOT excluded.upload_id;
