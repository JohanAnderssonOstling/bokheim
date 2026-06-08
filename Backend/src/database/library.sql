

-- name: get_book_paths?
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '__root__' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name,
        CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END
    FROM dir d
    JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id
    WHERE d.deleted_at IS NULL
)
SELECT CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END AS file_path
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
WHERE bd.deleted_at IS NULL;

-- name: get_book_assets?
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '__root__' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name,
        CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END
    FROM dir d
    JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id
    WHERE d.deleted_at IS NULL
)
SELECT
    bd.book_id,
    bd.local_hash,
    CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END AS file_path,
    bd.is_downloaded
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
WHERE bd.deleted_at IS NULL;

-- name: get_book_paths_without_title?
SELECT
    b.id AS book_id,
    CASE
        WHEN dp.path = ''
            THEN bd.file_name
        ELSE dp.path || '/' || bd.file_name
    END AS full_path
FROM book b
JOIN book_dir bd ON bd.book_id = b.id
JOIN dir_paths dp ON dp.id = bd.dir_id
WHERE b.deleted_at IS NULL
  AND (b.title IS NULL OR b.author_id IS NULL)
  AND bd.deleted_at IS NULL;




-- name: get_dir_paths?
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '__root__' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name,
        CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END
    FROM dir d
    JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id
    WHERE d.deleted_at IS NULL
)
SELECT path FROM dir_paths ORDER BY path;

-- name: get_sub_dirs?
-- param: dir_id: &str
select  directory.id,        directory.name
from    dir as directory
where   directory.parent_id = :dir_id
        and   directory.id != :dir_id
        and   directory.deleted_at is null;

-- name: get_books_in_dir?
-- param: dir_id: &str
SELECT book.id, book.title, author.name
from book_dir as book_mapping
join book as book
    on book.id = book_mapping.book_id
left join author as author
    on author.id = book.author_id
where book_mapping.dir_id = :dir_id
  and book_mapping.deleted_at is null
  and book.deleted_at is null;

-- name: get_authors?
SELECT
    author.name,
    COUNT(DISTINCT book.id) AS book_count
FROM author
JOIN book ON book.author_id = author.id
JOIN book_dir ON book_dir.book_id = book.id
WHERE book.deleted_at IS NULL
  AND book_dir.deleted_at IS NULL
  AND author.name IS NOT NULL
  AND TRIM(author.name) != ''
GROUP BY author.id, author.name
ORDER BY LOWER(author.name);

-- name: get_author_books?
-- param: author_name: &str
SELECT
    dp.id AS dir_id,
    CASE WHEN dp.path = '' THEN '(Root)' ELSE dp.path END AS folder_path,
    book.id AS book_id,
    book.title AS title,
    author.name AS author_name
FROM book
JOIN author ON author.id = book.author_id
JOIN book_dir ON book_dir.book_id = book.id
JOIN dir_paths dp ON dp.id = book_dir.dir_id
WHERE book_dir.deleted_at IS NULL
  AND book.deleted_at IS NULL
  AND author.name = :author_name
ORDER BY folder_path, LOWER(book.title);

-- name: insert_book!
-- param: id:   &str
-- param: title: &str
-- param: timestamp: i64
INSERT INTO book    (id, title, read_pos, read_progress, last_read, added_at, deleted_at)
VALUES              (:id,:title, "0", 0, 0, :timestamp, NULL)
ON CONFLICT(id) DO UPDATE SET
    title = excluded.title,
    added_at = excluded.added_at,
    deleted_at = NULL;

-- name: first_book_insert!
-- param: id: &str
-- param: timestamp: i64
INSERT OR IGNORE INTO book    (id, added_at)
VALUES              (:id, :timestamp);

-- name: update_book_title!
-- param: id: &str
-- param: title: &str
UPDATE book SET title = :title WHERE id = :id;

-- name: upsert_dir!
-- param: id: &str
-- param: parent_id: &str
-- param: name: &str
INSERT INTO dir (id, parent_id, name, deleted_at)
VALUES (:id, :parent_id, :name, NULL)
ON CONFLICT(id) DO UPDATE SET
    parent_id = excluded.parent_id,
    name = excluded.name,
    deleted_at = NULL;


-- name: get_book_dir_by_hash?
-- param: local_hash: &str
SELECT book_id, dir_id, file_name  FROM book_dir
WHERE local_hash = :local_hash;

-- name: get_book_hashes?
-- param: book_id: &str
SELECT local_hash FROM book_dir
WHERE book_id = :book_id AND deleted_at IS NULL;

-- name: insert_book_dir!
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
-- param: local_hash: &str
-- param: is_downloaded: i32
INSERT INTO book_dir    (dir_id, book_id, file_name, local_hash, is_downloaded, deleted_at)
VALUES                  (:dir_id,:book_id,:file_name,:local_hash,:is_downloaded, NULL)
ON CONFLICT(local_hash) DO UPDATE SET
    deleted_at = NULL,
    dir_id = excluded.dir_id,
    book_id = excluded.book_id,
    file_name = excluded.file_name,
    is_downloaded = max(book_dir.is_downloaded, excluded.is_downloaded),
    local_hash = excluded.local_hash;

-- name: get_recently_read?
-- param: limit: i32
SELECT
    book.id,
    book.title,
    author.name AS author,
    CASE
        WHEN book.read_progress > 0 THEN book.read_progress
        WHEN book.read_pos IS NOT NULL
            AND book.read_pos != ''
            AND book.read_pos != '0'
            AND CAST(book.read_pos AS REAL) > 0
            AND CAST(book.read_pos AS REAL) <= 1
            THEN CAST(book.read_pos AS REAL) * 100
        WHEN book.read_pos IS NOT NULL
            AND book.read_pos != ''
            AND book.read_pos != '0'
            AND CAST(book.read_pos AS REAL) > 1
            AND CAST(book.read_pos AS REAL) <= 100
            THEN CAST(book.read_pos AS REAL)
        ELSE 0
    END AS progress_pct,
    book.last_read
FROM book
LEFT JOIN author ON author.id = book.author_id
WHERE book.deleted_at IS NULL
  AND book.last_read > 0 
ORDER BY book.last_read DESC 
LIMIT :limit;

-- name: get_recently_added?
-- param: limit: i32
SELECT
    book.id,
    book.title,
    author.name AS author,
    CASE
        WHEN book.read_progress > 0 THEN book.read_progress
        WHEN book.read_pos IS NOT NULL
            AND book.read_pos != ''
            AND book.read_pos != '0'
            AND CAST(book.read_pos AS REAL) > 0
            AND CAST(book.read_pos AS REAL) <= 1
            THEN CAST(book.read_pos AS REAL) * 100
        WHEN book.read_pos IS NOT NULL
            AND book.read_pos != ''
            AND book.read_pos != '0'
            AND CAST(book.read_pos AS REAL) > 1
            AND CAST(book.read_pos AS REAL) <= 100
            THEN CAST(book.read_pos AS REAL)
        ELSE 0
    END AS progress_pct,
    book.added_at
FROM book
LEFT JOIN author ON author.id = book.author_id
WHERE book.deleted_at IS NULL
  AND book.added_at IS NOT NULL AND book.added_at > 0
ORDER BY book.added_at DESC 
LIMIT :limit;

-- name: count_all_books?
SELECT COUNT(*) FROM book WHERE deleted_at IS NULL;

-- name: count_books_with_added_at?
SELECT COUNT(*) FROM book WHERE deleted_at IS NULL AND added_at IS NOT NULL AND added_at > 0;

-- name: get_book_file_path?
-- param: book_id: &str
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '__root__' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name,
        CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END
    FROM dir d
    JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id
    WHERE d.deleted_at IS NULL
)
SELECT CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
WHERE bd.book_id = :book_id AND bd.deleted_at IS NULL
LIMIT 1;

-- name: get_book_file_path_components?
-- param: book_id: &str
WITH RECURSIVE path_chain(id, parent_id, name, level) AS (
    -- Start from the directory containing the book
    SELECT d.id, d.parent_id, d.name, 0 AS level
    FROM dir d
    JOIN book_dir bd ON bd.dir_id = d.id
    WHERE bd.book_id = :book_id AND bd.deleted_at IS NULL
    UNION ALL
    -- Walk up to parent directories
    SELECT d.id, d.parent_id, d.name, pc.level + 1
    FROM dir d
    JOIN path_chain pc ON pc.parent_id = d.id
    WHERE d.deleted_at IS NULL AND d.id != '__root__'
)
SELECT path_component
FROM (
    -- Get all directory components in order (root to leaf), excluding __root__
    SELECT name AS path_component, level
    FROM path_chain
    WHERE id != '__root__'
    UNION ALL
    -- Add the filename as the final component
    SELECT file_name AS path_component, -1 AS level
    FROM book_dir
    WHERE book_id = :book_id AND deleted_at IS NULL
)
ORDER BY level DESC;

-- name: get_book_reading_position?
-- param: book_id: &str
SELECT read_pos FROM book WHERE id = :book_id AND deleted_at IS NULL;

-- name: is_book_downloaded?
-- param: book_id: &str
SELECT COUNT(*) > 0
FROM book_dir
WHERE book_id = :book_id
  AND deleted_at is null
  AND is_downloaded = 1;

-- name: update_reading_position!
-- param: book_id: &str
-- param: cfi: &str
UPDATE book SET read_pos = :cfi WHERE id = :book_id AND deleted_at IS NULL;

-- name: update_last_read!
-- param: book_id: &str
-- param: timestamp: i64
UPDATE book SET last_read = :timestamp WHERE id = :book_id AND deleted_at IS NULL;

-- name: get_book_dir_entry_by_path?
-- param: file_path: &str
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '__root__' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name,
        CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END
    FROM dir d
    JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id
    WHERE d.deleted_at IS NULL
)
SELECT bd.book_id, bd.dir_id, bd.file_name
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
WHERE CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END = :file_path
  AND bd.deleted_at IS NULL
LIMIT 1;

-- name: mark_dir_deleted!
-- param: dir_id: &str
UPDATE dir
SET deleted_at = 1
WHERE id = :dir_id
  AND deleted_at IS NULL;

-- name: mark_book_dir_deleted!
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
UPDATE book_dir
SET deleted_at = 1
WHERE dir_id = :dir_id AND book_id = :book_id AND file_name = :file_name
  AND deleted_at IS NULL;

-- name: set_book_downloaded!
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
-- param: is_downloaded: i32
UPDATE book_dir
SET is_downloaded = :is_downloaded
WHERE dir_id = :dir_id AND book_id = :book_id AND file_name = :file_name
  AND deleted_at IS NULL;

-- Event sourcing queries
-- name: get_events_after_local_seq?
-- param: local_seq: u64
SELECT entity_id, event_type, payload, timestamp, local_seq
FROM event_log
WHERE local_seq > :local_seq
ORDER BY local_seq ASC;

-- name: get_unpushed_events?
-- param: last_local_seq_sent: u64
SELECT entity_id, event_type, payload, timestamp, local_seq, hlc_pt, hlc_l
FROM event_log
WHERE local_seq > :last_local_seq_sent
ORDER BY local_seq ASC;

-- name: get_sync_metadata_value?
-- param: key: &str
SELECT value FROM sync_metadata WHERE key = :key;

-- name: set_sync_metadata!
-- param: key: &str
-- param: value: &str
INSERT OR REPLACE INTO sync_metadata (key, value) VALUES (:key, :value);






-- name: count_book_paths?
-- param: book_id: &str
SELECT COUNT(*)
FROM book_dir
WHERE book_id = :book_id AND deleted_at IS NULL;

-- name: mark_book_deleted!
-- param: book_id: &str
-- param: ts: i64
UPDATE book
SET deleted_at = :ts
WHERE id = :book_id
  AND deleted_at IS NULL;

-- name: get_all_reading_positions?
SELECT id, read_pos FROM book WHERE deleted_at IS NULL AND read_pos IS NOT NULL AND read_pos != '0';

-- transfer_jobs
-- name: upsert_transfer_job!
-- param: kind: &str
-- param: key1: &str
-- param: rel_path: &str
-- param: local_path: &str
-- param: book_id: &str
-- param: now_ts: i64
INSERT INTO transfer_jobs (kind, key1, rel_path, local_path, book_id, created_at, updated_at)
VALUES (:kind, :key1, :rel_path, :local_path, :book_id, :now_ts, :now_ts)
ON CONFLICT(kind, key1, rel_path, local_path, book_id) DO UPDATE SET
    updated_at = :now_ts;

-- name: claim_due_transfer_job?
-- param: now_ts: i64
UPDATE transfer_jobs
SET claimed_at = :now_ts, updated_at = :now_ts
WHERE id = (
    SELECT id
    FROM transfer_jobs
    WHERE claimed_at IS NULL
      AND next_retry_at <= :now_ts
    ORDER BY created_at ASC
    LIMIT 1
)
RETURNING id, kind, key1, rel_path, local_path, book_id, attempts;

-- name: complete_transfer_job!
-- param: id: i64
DELETE FROM transfer_jobs
WHERE id = :id;

-- name: fail_transfer_job!
-- param: id: i64
-- param: last_error: &str
-- param: next_retry_at: i64
-- param: now_ts: i64
UPDATE transfer_jobs
SET attempts = attempts + 1,
    claimed_at = NULL,
    last_error = :last_error,
    next_retry_at = :next_retry_at,
    updated_at = :now_ts
WHERE id = :id;

-- name: reset_stale_claims!
-- param: stale_before: i64
UPDATE transfer_jobs
SET claimed_at = NULL
WHERE claimed_at IS NOT NULL
  AND claimed_at < :stale_before;

-- name: delete_unclaimed_transfer_jobs!
DELETE FROM transfer_jobs
WHERE claimed_at IS NULL;

-- name: get_unclaimed_transfer_jobs?
SELECT id, kind, key1, rel_path, local_path, book_id
FROM transfer_jobs
WHERE claimed_at IS NULL;

-- name: delete_transfer_job_by_id!
-- param: id: i64
DELETE FROM transfer_jobs
WHERE id = :id;

-- blob_state / blob_local_ref
-- name: upsert_blob_local_ref!
-- param: kind: &str
-- param: hash: &str
-- param: path: &str
-- param: exists_flag: i32
-- param: last_seen: i64
INSERT INTO blob_local_ref(kind, hash, path, exists_flag, last_seen)
VALUES (:kind, :hash, :path, :exists_flag, :last_seen)
ON CONFLICT(kind, hash, path) DO UPDATE SET
    exists_flag = :exists_flag,
    last_seen = :last_seen;

-- name: upsert_blob_state!
-- param: kind: &str
-- param: hash: &str
-- param: state: i32
-- param: last_checked: i64
INSERT INTO blob_state(kind, hash, state, last_checked)
VALUES (:kind, :hash, :state, :last_checked)
ON CONFLICT(kind, hash) DO UPDATE SET
    state = :state,
    last_checked = :last_checked;

-- name: get_blob_local_presence?
-- param: kind: &str
-- param: hash: &str
SELECT COALESCE(MAX(exists_flag), 0)
FROM blob_local_ref
WHERE kind = :kind
  AND hash = :hash;

-- name: set_blob_state_checked!
-- param: kind: &str
-- param: hash: &str
-- param: state: i32
-- param: last_checked: i64
UPDATE blob_state
SET state = :state,
    last_checked = :last_checked,
    last_error = NULL
WHERE kind = :kind
  AND hash = :hash;

-- name: set_blob_state_error!
-- param: kind: &str
-- param: hash: &str
-- param: last_error: &str
-- param: last_checked: i64
INSERT INTO blob_state(kind, hash, state, last_checked, last_error)
VALUES (:kind, :hash, 0, :last_checked, :last_error)
ON CONFLICT(kind, hash) DO UPDATE SET
    last_checked = :last_checked,
    last_error = :last_error;

-- name: get_blob_hashes_by_state?
-- param: kind: &str
-- param: state: i32
SELECT hash
FROM blob_state
WHERE kind = :kind
  AND state = :state;

-- name: get_blob_local_path?
-- param: kind: &str
-- param: hash: &str
SELECT path
FROM blob_local_ref
WHERE kind = :kind
  AND hash = :hash
  AND exists_flag = 1
ORDER BY last_seen DESC
LIMIT 1;

-- name: get_blob_any_path?
-- param: kind: &str
-- param: hash: &str
SELECT path
FROM blob_local_ref
WHERE kind = :kind
  AND hash = :hash
ORDER BY exists_flag DESC, last_seen DESC, path ASC
LIMIT 1;

-- name: get_book_paths_by_local_hash?
-- param: local_hash: &str
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (
    SELECT id, parent_id, name, '/' AS path
    FROM dir
    WHERE id = '__root__' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name,
        CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END
    FROM dir d
    JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id
    WHERE d.deleted_at IS NULL
)
SELECT CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END AS file_path
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
WHERE bd.local_hash = :local_hash
  AND bd.deleted_at IS NULL
ORDER BY file_path ASC;
