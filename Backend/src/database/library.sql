

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
WHERE b.title IS NULL
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
        and   directory.deleted_at is null;

-- name: get_books_in_dir?
-- param: dir_id: &str
SELECT book.id, book.title
from book_dir as book_mapping
join book as book
    on book.id = book_mapping.book_id
where book_mapping.dir_id = :dir_id
  and book_mapping.deleted_at is null;

-- name: insert_book!
-- param: id:   &str
-- param: title: &str
-- param: timestamp: i64
INSERT INTO book    (id, title, read_pos, read_progress, last_read, added_at)
VALUES              (:id,:title, "0", 0, 0, :timestamp)
ON CONFLICT(id) DO UPDATE SET
    title = excluded.title,
    added_at = excluded.added_at;

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
SELECT id, title FROM book 
WHERE last_read > 0 
ORDER BY last_read DESC 
LIMIT :limit;

-- name: get_recently_added?
-- param: limit: i32
SELECT id, title, added_at FROM book 
WHERE added_at IS NOT NULL AND added_at > 0
ORDER BY added_at DESC 
LIMIT :limit;

-- name: count_all_books?
SELECT COUNT(*) FROM book;

-- name: count_books_with_added_at?
SELECT COUNT(*) FROM book WHERE added_at IS NOT NULL AND added_at > 0;

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

-- name: get_book_reading_position?
-- param: book_id: &str
SELECT read_pos FROM book WHERE id = :book_id;

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
UPDATE book SET read_pos = :cfi WHERE id = :book_id;

-- name: update_last_read!
-- param: book_id: &str
-- param: timestamp: i64
UPDATE book SET last_read = :timestamp WHERE id = :book_id;

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

-- name: set_book_uploaded!
-- param: book_id: &str
-- param: uploaded: i32
UPDATE book
SET uploaded = :uploaded
WHERE id = :book_id;

-- name: set_book_thumbnail_uploaded!
-- param: book_id: &str
-- param: uploaded_thumbnail: i32
UPDATE book
SET uploaded_thumbnail = :uploaded_thumbnail
WHERE id = :book_id;

-- Event sourcing queries
-- name: get_events_after_local_seq?
-- param: local_seq: u64
SELECT entity_id, event_type, payload, timestamp, local_seq, event_kind
FROM event_log
WHERE local_seq > :local_seq
ORDER BY local_seq ASC;

-- name: get_unpushed_events?
-- param: last_local_seq_sent: u64
SELECT entity_id, event_type, payload, timestamp, local_seq, event_kind
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

-- name: delete_book_by_id!
-- param: book_id: &str
DELETE FROM book WHERE id = :book_id;

-- name: get_all_reading_positions?
SELECT id, read_pos FROM book WHERE read_pos IS NOT NULL AND read_pos != '0';
