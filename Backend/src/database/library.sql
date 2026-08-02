

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
    bd.content_hash,
    CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END AS file_path,
    bd.is_downloaded
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
WHERE bd.deleted_at IS NULL AND bd.content_hash != '';

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
WITH RECURSIVE descendants(root_id, id) AS (
    SELECT directory.id, directory.id
    FROM dir AS directory
    WHERE directory.parent_id = :dir_id
      AND directory.id != :dir_id
      AND directory.deleted_at IS NULL
    UNION
    SELECT descendants.root_id, child.id
    FROM descendants
    JOIN dir AS child ON child.parent_id = descendants.id
    WHERE child.id != descendants.id
      AND child.deleted_at IS NULL
)
SELECT
    root.id,
    root.name,
    COUNT(DISTINCT book.id) AS book_count,
    COUNT(DISTINCT CASE WHEN placement.is_downloaded = 1 THEN book.id END) AS downloaded_book_count
FROM dir AS root
JOIN descendants ON descendants.root_id = root.id
LEFT JOIN book_dir AS placement ON placement.dir_id = descendants.id AND placement.deleted_at IS NULL
LEFT JOIN book ON book.id = placement.book_id AND book.deleted_at IS NULL
WHERE root.parent_id = :dir_id
  AND root.id != :dir_id
  AND root.deleted_at IS NULL
GROUP BY root.id, root.name;

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

-- name: get_descendant_book_ids?
-- param: dir_id: &str
WITH RECURSIVE subtree(id) AS (
    SELECT id FROM dir WHERE id = :dir_id AND deleted_at IS NULL
    UNION
    SELECT child.id
    FROM dir child
    JOIN subtree parent ON child.parent_id = parent.id
    WHERE child.id != child.parent_id
      AND child.deleted_at IS NULL
)
SELECT DISTINCT placement.book_id
FROM book_dir placement
JOIN subtree ON subtree.id = placement.dir_id
JOIN book ON book.id = placement.book_id
WHERE placement.deleted_at IS NULL
  AND book.deleted_at IS NULL
ORDER BY placement.book_id;

-- name: get_folder_download_counts?
-- param: dir_id: &str
WITH RECURSIVE subtree(id) AS (
    SELECT id FROM dir WHERE id = :dir_id AND deleted_at IS NULL
    UNION
    SELECT child.id
    FROM dir child
    JOIN subtree parent ON child.parent_id = parent.id
    WHERE child.id != child.parent_id
      AND child.deleted_at IS NULL
)
SELECT
    COUNT(DISTINCT placement.book_id),
    COUNT(DISTINCT CASE WHEN placement.is_downloaded = 1 THEN placement.book_id END)
FROM book_dir placement
JOIN subtree ON subtree.id = placement.dir_id
JOIN book ON book.id = placement.book_id
WHERE placement.deleted_at IS NULL
  AND book.deleted_at IS NULL;

-- name: get_book_ancestor_dir_ids?
-- param: book_id: &str
WITH RECURSIVE ancestors(id) AS (
    SELECT placement.dir_id
    FROM book_dir placement
    WHERE placement.book_id = :book_id
      AND placement.deleted_at IS NULL
    UNION
    SELECT directory.parent_id
    FROM dir directory
    JOIN ancestors ON ancestors.id = directory.id
    WHERE directory.id != '__root__'
      AND directory.deleted_at IS NULL
)
SELECT id FROM ancestors ORDER BY id;

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
VALUES              (:id,:title, NULL, 0, 0, :timestamp, NULL)
ON CONFLICT(id) DO UPDATE SET
    title = COALESCE(book.title, excluded.title),
    added_at = excluded.added_at,
    deleted_at = NULL;

-- name: get_book_version?
-- param: id: &str
SELECT hlc_pt, hlc_l, hlc_actor, hlc_seq, deleted_at IS NOT NULL, hlc_event
FROM book
WHERE id = :id;

-- name: apply_remote_book_added!
-- param: id: &str
-- param: title: &str
-- param: timestamp: i64
-- param: hlc_pt: i64
-- param: hlc_l: i64
-- param: hlc_actor: &str
-- param: hlc_seq: i64
-- param: hlc_event: &str
INSERT INTO book (id, title, read_pos, read_progress, last_read, added_at, hlc_pt, hlc_l, hlc_actor, hlc_seq, hlc_event, deleted_at)
VALUES (:id, :title, NULL, 0, 0, :timestamp, :hlc_pt, :hlc_l, :hlc_actor, :hlc_seq, :hlc_event, NULL)
ON CONFLICT(id) DO UPDATE SET
    title = COALESCE(book.title, excluded.title),
    added_at = excluded.added_at,
    hlc_pt = excluded.hlc_pt,
    hlc_l = excluded.hlc_l,
    hlc_actor = excluded.hlc_actor,
    hlc_seq = excluded.hlc_seq,
    hlc_event = excluded.hlc_event,
    deleted_at = NULL;

-- name: apply_remote_book_deleted!
-- param: id: &str
-- param: timestamp: i64
-- param: hlc_pt: i64
-- param: hlc_l: i64
-- param: hlc_actor: &str
-- param: hlc_seq: i64
-- param: hlc_event: &str
-- Upsert a tombstone even when this device has never observed the book. Without
-- a row carrying the winning version, a later stale add could resurrect it.
INSERT INTO book (id, added_at, hlc_pt, hlc_l, hlc_actor, hlc_seq, hlc_event, deleted_at)
VALUES (:id, :timestamp, :hlc_pt, :hlc_l, :hlc_actor, :hlc_seq, :hlc_event, :timestamp)
ON CONFLICT(id) DO UPDATE SET
    hlc_pt = excluded.hlc_pt,
    hlc_l = excluded.hlc_l,
    hlc_actor = excluded.hlc_actor,
    hlc_seq = excluded.hlc_seq,
    hlc_event = excluded.hlc_event,
    deleted_at = excluded.deleted_at;

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
INSERT INTO dir (id, parent_id, name, intent_parent_id, intent_name, intent_deleted, deleted_at)
VALUES (:id, :parent_id, :name, :parent_id, :name, 0, NULL)
ON CONFLICT(id) DO UPDATE SET
    parent_id = excluded.parent_id,
    name = excluded.name,
    intent_parent_id = excluded.parent_id,
    intent_name = excluded.name,
    intent_deleted = 0,
    deleted_at = NULL;

-- name: apply_remote_dir_state!
-- param: id: &str
-- param: parent_id: &str
-- param: name: &str
-- param: deleted: bool
-- param: hlc_pt: i64
-- param: hlc_l: i64
-- param: hlc_actor: &str
-- param: hlc_seq: i64
-- param: hlc_event: &str
INSERT INTO dir (id, parent_id, name, intent_parent_id, intent_name, intent_deleted, deleted_at, hlc_pt, hlc_l, hlc_actor, hlc_seq, hlc_event)
VALUES (:id, :parent_id, :name, :parent_id, :name, :deleted, 1, :hlc_pt, :hlc_l, :hlc_actor, :hlc_seq, :hlc_event)
ON CONFLICT(id) DO UPDATE SET
    intent_parent_id = excluded.intent_parent_id,
    intent_name = excluded.intent_name,
    intent_deleted = excluded.intent_deleted,
    hlc_pt = excluded.hlc_pt,
    hlc_l = excluded.hlc_l,
    hlc_actor = excluded.hlc_actor,
    hlc_seq = excluded.hlc_seq,
    hlc_event = excluded.hlc_event;

-- name: get_dir_version?
-- param: id: &str
SELECT hlc_pt, hlc_l, hlc_actor, hlc_seq, COALESCE(intent_deleted, deleted_at IS NOT NULL), hlc_event
FROM dir WHERE id = :id;

-- name: get_dir_path_components?
-- param: dir_id: &str
WITH RECURSIVE path_chain(id, parent_id, name, level) AS (
    SELECT id, parent_id, name, 0 AS level
    FROM dir
    WHERE id = :dir_id AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name, pc.level + 1
    FROM dir d
    JOIN path_chain pc ON pc.parent_id = d.id
    WHERE d.deleted_at IS NULL AND d.id != '__root__'
)
SELECT id, name
FROM path_chain
WHERE id != '__root__'
ORDER BY level DESC;


-- name: get_book_dir_by_hash?
-- param: local_hash: &str
SELECT book_id, dir_id, file_name, content_hash  FROM book_dir
WHERE local_hash = :local_hash;

-- name: get_book_hashes?
-- param: book_id: &str
SELECT content_hash FROM book_dir
WHERE book_id = :book_id AND deleted_at IS NULL AND content_hash != '';

-- name: insert_book_dir!
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
-- param: local_hash: &str
-- param: content_hash: &str
-- param: last_scan: i64
-- param: is_downloaded: i32
INSERT INTO book_dir    (dir_id, book_id, file_name, local_hash, content_hash, last_scan, is_downloaded, intent_deleted, deleted_at)
VALUES                  (:dir_id,:book_id,:file_name,:local_hash,:content_hash,:last_scan,:is_downloaded, 0, NULL)
ON CONFLICT(dir_id, book_id, file_name) DO UPDATE SET
    intent_deleted = 0,
    deleted_at = NULL,
    content_hash = excluded.content_hash,
    -- max() so a remote apply (last_scan 0) can't lower a live scan stamp and
    -- make the next sweep delete a present file.
    last_scan = max(book_dir.last_scan, excluded.last_scan),
    is_downloaded = max(book_dir.is_downloaded, excluded.is_downloaded),
    local_hash = excluded.local_hash;

-- name: mark_vanished_book_dirs_deleted!
-- param: scan_id: i64
-- param: grace: i64
-- param: deleted_at: i64
-- Soft-delete locally-materialized placements missing for at least `grace`
-- consecutive scans. is_downloaded = 1 restricts this to files that were on disk;
-- last_scan > 0 to a file an actual scan once confirmed present (so a freshly
-- downloaded, not-yet-scanned file isn't deleted). The grace window rides out
-- transient misses (a file briefly locked, a partial mount). Fires
-- trig_book_dir_deleted per row, emitting book_dir_removed for sync, which
-- propagates the deletion (and the local file) to other devices.
UPDATE book_dir
SET deleted_at = :deleted_at,
    intent_deleted = 1
WHERE deleted_at IS NULL
  AND is_downloaded = 1
  AND last_scan > 0
  AND (:scan_id - last_scan) >= :grace;

-- name: count_pending_vanish_candidates?
-- param: scan_id: i64
-- param: grace: i64
-- Placements missing now but not yet past the grace window. While any exist a
-- re-scan is scheduled so the miss count advances even if nothing else triggers a
-- scan (deletions are event-driven, so one removal would otherwise stall at one miss).
SELECT COUNT(*) FROM book_dir
WHERE deleted_at IS NULL
  AND is_downloaded = 1
  AND last_scan > 0
  AND last_scan < :scan_id
  AND (:scan_id - last_scan) < :grace;

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
  -- A book whose every placement was removed (e.g. its file was deleted on
  -- another device) keeps its metadata row but is un-openable. Require a live
  -- placement so it drops out of the carousel and reappears if one returns.
  AND EXISTS (SELECT 1 FROM book_dir bd WHERE bd.book_id = book.id AND bd.deleted_at IS NULL)
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
  -- See get_recently_read: exclude books with no live placement.
  AND EXISTS (SELECT 1 FROM book_dir bd WHERE bd.book_id = book.id AND bd.deleted_at IS NULL)
ORDER BY book.added_at DESC
LIMIT :limit;

-- name: count_all_books?
-- Counts only books with a live placement, matching what the carousels display.
SELECT COUNT(*) FROM book
WHERE deleted_at IS NULL
  AND EXISTS (SELECT 1 FROM book_dir bd WHERE bd.book_id = book.id AND bd.deleted_at IS NULL);

-- name: count_books_with_added_at?
SELECT COUNT(*) FROM book
WHERE deleted_at IS NULL AND added_at IS NOT NULL AND added_at > 0
  AND EXISTS (SELECT 1 FROM book_dir bd WHERE bd.book_id = book.id AND bd.deleted_at IS NULL);

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

-- name: is_book_download_requested?
-- param: book_id: &str
SELECT
    EXISTS (
        SELECT 1 FROM book_download_requests
        WHERE book_id = :book_id
    )
    OR EXISTS (
        SELECT 1
        FROM book_dir placement
        JOIN asset_requests request
          ON request.kind = 'book'
         AND request.hash = placement.content_hash
        WHERE placement.book_id = :book_id
          AND placement.deleted_at IS NULL
    );

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
SET deleted_at = 1,
    intent_deleted = 1
WHERE id = :dir_id
  AND deleted_at IS NULL;

-- name: get_dir_delete_file_paths?
-- param: dir_id: &str
WITH RECURSIVE subtree(id, depth) AS (
    SELECT id, 0
    FROM dir
    WHERE id = :dir_id AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, subtree.depth + 1
    FROM dir d
    JOIN subtree ON d.parent_id = subtree.id AND d.id != subtree.id
    WHERE d.deleted_at IS NULL
)
SELECT CASE
    WHEN dp.path = '' THEN bd.file_name
    ELSE dp.path || '/' || bd.file_name
END
FROM subtree
JOIN dir_paths dp ON dp.id = subtree.id
JOIN book_dir bd ON bd.dir_id = subtree.id
WHERE bd.deleted_at IS NULL
  AND bd.is_downloaded = 1
ORDER BY subtree.depth DESC;

-- name: get_dir_delete_dir_paths?
-- param: dir_id: &str
WITH RECURSIVE subtree(id, depth) AS (
    SELECT id, 0
    FROM dir
    WHERE id = :dir_id AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, subtree.depth + 1
    FROM dir d
    JOIN subtree ON d.parent_id = subtree.id AND d.id != subtree.id
    WHERE d.deleted_at IS NULL
)
SELECT dp.path, subtree.id
FROM subtree
JOIN dir_paths dp ON dp.id = subtree.id
ORDER BY depth DESC;

-- name: tombstone_book_dirs_in_subtree!
-- param: dir_id: &str
-- param: deleted_at: i64
WITH RECURSIVE subtree(id) AS (
    SELECT id
    FROM dir
    WHERE id = :dir_id AND deleted_at IS NULL
    UNION ALL
    SELECT d.id
    FROM dir d
    JOIN subtree ON d.parent_id = subtree.id AND d.id != subtree.id
    WHERE d.deleted_at IS NULL
)
UPDATE book_dir
SET deleted_at = :deleted_at,
    intent_deleted = 1
WHERE deleted_at IS NULL
  AND dir_id IN (SELECT id FROM subtree);

-- name: tombstone_dirs_in_subtree!
-- param: dir_id: &str
-- param: deleted_at: i64
WITH RECURSIVE subtree(id) AS (
    SELECT id
    FROM dir
    WHERE id = :dir_id AND deleted_at IS NULL
    UNION ALL
    SELECT d.id
    FROM dir d
    JOIN subtree ON d.parent_id = subtree.id AND d.id != subtree.id
    WHERE d.deleted_at IS NULL
)
UPDATE dir
SET deleted_at = :deleted_at,
    intent_deleted = 1
WHERE deleted_at IS NULL
  AND id IN (SELECT id FROM subtree);

-- name: mark_book_dir_deleted!
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
UPDATE book_dir
SET deleted_at = 1,
    intent_deleted = 1
WHERE dir_id = :dir_id AND book_id = :book_id AND file_name = :file_name
  AND deleted_at IS NULL;

-- name: get_book_dir_hlc?
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
SELECT hlc_pt, hlc_l, hlc_actor, hlc_seq, COALESCE(intent_deleted, deleted_at IS NOT NULL), hlc_event FROM book_dir
WHERE dir_id = :dir_id AND book_id = :book_id AND file_name = :file_name;

-- name: resolve_removed_book_dir?
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
-- Resolves a tombstoned placement's file path AND content hash. Returns nothing
-- unless the placement is currently deleted, so a placement an add resurrected
-- later in the same pull batch is left alone. The content hash lets the caller
-- relocate the file to another live placement of the same content (a move)
-- instead of deleting it.
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
    CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END AS file_path,
    bd.content_hash
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
WHERE bd.dir_id = :dir_id AND bd.book_id = :book_id AND bd.file_name = :file_name
  AND bd.deleted_at IS NOT NULL
LIMIT 1;

-- Remote LWW apply of book_dir add/remove. The caller gates on the HLC first
-- (see apply_remote_book_dir), so these set the placement's hlc unconditionally.
-- change_origin is 'remote' during apply, so the book_dir triggers don't fire.
-- name: apply_remote_book_dir_added!
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
-- param: content_hash: &str
-- param: hlc_pt: i64
-- param: hlc_l: i64
-- param: hlc_actor: &str
-- param: hlc_seq: i64
-- param: hlc_event: &str
INSERT INTO book_dir (dir_id, book_id, file_name, local_hash, content_hash, last_scan, is_downloaded, hlc_pt, hlc_l, hlc_actor, hlc_seq, hlc_event, intent_deleted, deleted_at)
VALUES (:dir_id, :book_id, :file_name, :content_hash, :content_hash, 0, 0, :hlc_pt, :hlc_l, :hlc_actor, :hlc_seq, :hlc_event, 0, NULL)
ON CONFLICT(dir_id, book_id, file_name) DO UPDATE SET
    intent_deleted = 0,
    deleted_at = NULL,
    content_hash = excluded.content_hash,
    local_hash = excluded.local_hash,
    is_downloaded = max(book_dir.is_downloaded, excluded.is_downloaded),
    hlc_pt = excluded.hlc_pt,
    hlc_l = excluded.hlc_l,
    hlc_actor = excluded.hlc_actor,
    hlc_seq = excluded.hlc_seq,
    hlc_event = excluded.hlc_event;

-- name: apply_remote_book_dir_removed!
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
-- param: tombstone_hash: &str
-- param: hlc_pt: i64
-- param: hlc_l: i64
-- param: hlc_actor: &str
-- param: hlc_seq: i64
-- param: hlc_event: &str
-- Upserts a tombstone (deleted_at set) even if the placement was never seen
-- locally, so a later lower-HLC add still loses the LWW comparison.
INSERT INTO book_dir (dir_id, book_id, file_name, local_hash, content_hash, last_scan, is_downloaded, hlc_pt, hlc_l, hlc_actor, hlc_seq, hlc_event, intent_deleted, deleted_at)
VALUES (:dir_id, :book_id, :file_name, :tombstone_hash, '', 0, 0, :hlc_pt, :hlc_l, :hlc_actor, :hlc_seq, :hlc_event, 1, 1)
ON CONFLICT(dir_id, book_id, file_name) DO UPDATE SET
    intent_deleted = 1,
    deleted_at = 1,
    hlc_pt = excluded.hlc_pt,
    hlc_l = excluded.hlc_l,
    hlc_actor = excluded.hlc_actor,
    hlc_seq = excluded.hlc_seq,
    hlc_event = excluded.hlc_event;

-- name: set_book_downloaded!
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
-- param: is_downloaded: i32
UPDATE book_dir
SET is_downloaded = :is_downloaded
WHERE dir_id = :dir_id AND book_id = :book_id AND file_name = :file_name
  AND deleted_at IS NULL;

-- name: update_book_dir_local_hash!
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
-- param: local_hash: &str
UPDATE book_dir
SET local_hash = :local_hash
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
SELECT event_id, entity_id, event_type, payload, timestamp, local_seq, hlc_pt, hlc_l
FROM event_log
WHERE local_seq > :last_local_seq_sent
ORDER BY local_seq ASC;

-- name: get_max_unpushed_hlc?
-- param: last_pushed_local_seq: u64
-- param: entity_id: &str
-- param: event_type: &str
SELECT hlc_pt, hlc_l, local_seq, event_id,
       CASE
           WHEN event_type = 'dir_state'
            AND COALESCE(json_extract(payload, '$.deleted'), 0) != 0
           THEN 1 ELSE 0
       END AS conflict_rank
FROM event_log
WHERE local_seq > :last_pushed_local_seq
  AND entity_id = :entity_id
  AND event_type = :event_type
ORDER BY hlc_pt DESC, hlc_l DESC, conflict_rank DESC, local_seq DESC
LIMIT 1;

-- name: get_sync_metadata_value?
-- param: key: &str
SELECT value FROM sync_metadata WHERE key = :key;

-- name: set_sync_metadata!
-- param: key: &str
-- param: value: &str
INSERT INTO sync_metadata (key, value) VALUES (:key, :value)
ON CONFLICT(key) DO UPDATE SET value = excluded.value;






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
-- param: origin: &str
-- param: now_ts: i64
INSERT INTO transfer_jobs (kind, key1, rel_path, local_path, book_id, origin, created_at, updated_at)
VALUES (:kind, :key1, :rel_path, :local_path, :book_id, :origin, :now_ts, :now_ts)
ON CONFLICT(kind, key1, rel_path, local_path, book_id) DO UPDATE SET
    origin = CASE WHEN excluded.origin = 'user_initiated' THEN 'user_initiated' ELSE transfer_jobs.origin END,
    updated_at = :now_ts;

-- name: claim_due_transfer_job?
-- param: now_ts: i64
-- param: claim_token: &str
-- param: allow_background_transfers: i32
UPDATE transfer_jobs
SET claimed_at = :now_ts, claim_token = :claim_token, updated_at = :now_ts
WHERE id = (
    SELECT id
    FROM transfer_jobs
    WHERE claimed_at IS NULL
      AND next_retry_at <= :now_ts
      AND (:allow_background_transfers = 1 OR origin = 'user_initiated')
    ORDER BY created_at ASC
    LIMIT 1
)
RETURNING id, kind, key1, rel_path, local_path, book_id, origin, attempts, claim_token;

-- name: complete_transfer_job!
-- param: id: i64
-- param: claim_token: &str
DELETE FROM transfer_jobs
WHERE id = :id AND claim_token = :claim_token;

-- name: get_next_transfer_retry_at?
SELECT MIN(next_retry_at)
FROM transfer_jobs
WHERE claimed_at IS NULL AND last_error IS NOT NULL;

-- name: fail_transfer_job!
-- param: id: i64
-- param: claim_token: &str
-- param: last_error: &str
-- param: next_retry_at: i64
-- param: now_ts: i64
UPDATE transfer_jobs
SET attempts = attempts + 1,
    claimed_at = NULL,
    claim_token = '',
    last_error = :last_error,
    next_retry_at = :next_retry_at,
    updated_at = :now_ts
WHERE id = :id AND claim_token = :claim_token;

-- name: heartbeat_transfer_job!
-- param: id: i64
-- param: claim_token: &str
-- param: now_ts: i64
UPDATE transfer_jobs
SET claimed_at=:now_ts, updated_at=:now_ts
WHERE id=:id AND claim_token=:claim_token AND claimed_at IS NOT NULL;

-- name: reset_stale_claims!
-- param: stale_before: i64
UPDATE transfer_jobs
SET claimed_at = NULL, claim_token = ''
WHERE claimed_at IS NOT NULL
  AND claimed_at < :stale_before;

-- name: delete_unclaimed_transfer_jobs!
DELETE FROM transfer_jobs
WHERE claimed_at IS NULL;

-- name: clear_transfer_jobs!
DELETE FROM transfer_jobs;

-- name: get_unclaimed_transfer_jobs?
SELECT id, kind, key1, rel_path, local_path, book_id, origin
FROM transfer_jobs
WHERE claimed_at IS NULL;

-- name: get_transfer_statuses?
SELECT id, kind, key1, rel_path, local_path, book_id, origin, attempts,
       claimed_at, next_retry_at, last_error, created_at, updated_at
FROM transfer_jobs
ORDER BY created_at ASC, id ASC;

-- name: delete_transfer_job_by_id!
-- param: id: i64
DELETE FROM transfer_jobs
WHERE id = :id;

-- Independent remote presence, local references, and durable transfer intent
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

-- name: mark_asset_remote_present!
-- param: kind: &str
-- param: hash: &str
-- param: last_checked: i64
INSERT INTO asset_remote_state(kind, hash, presence, last_checked, last_error)
VALUES (:kind, :hash, 2, :last_checked, NULL)
ON CONFLICT(kind, hash) DO UPDATE SET
    presence = 2,
    last_checked = :last_checked,
    last_error = NULL;

-- name: mark_asset_remote_missing!
-- param: kind: &str
-- param: hash: &str
-- param: last_checked: i64
INSERT INTO asset_remote_state(kind, hash, presence, last_checked, last_error)
VALUES (:kind, :hash, 1, :last_checked, NULL)
ON CONFLICT(kind, hash) DO UPDATE SET
    presence = 1,
    last_checked = :last_checked,
    last_error = NULL;

-- name: set_asset_remote_error!
-- param: kind: &str
-- param: hash: &str
-- param: last_error: &str
-- param: last_checked: i64
INSERT INTO asset_remote_state(kind, hash, presence, last_checked, last_error)
VALUES (:kind, :hash, 0, :last_checked, :last_error)
ON CONFLICT(kind, hash) DO UPDATE SET
    last_checked = :last_checked,
    last_error = :last_error;

-- name: get_locally_present_blob_hashes?
-- param: kind: &str
SELECT DISTINCT hash
FROM blob_local_ref
WHERE kind = :kind
  AND exists_flag = 1;

-- name: get_blob_local_presence?
-- param: kind: &str
-- param: hash: &str
SELECT COALESCE(MAX(exists_flag), 0)
FROM blob_local_ref
WHERE kind = :kind
  AND hash = :hash;

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

-- name: add_asset_request!
-- param: kind: &str
-- param: hash: &str
-- param: origin: &str
-- param: created_at: i64
INSERT INTO asset_requests(kind, hash, origin, created_at)
VALUES (:kind, :hash, :origin, :created_at)
ON CONFLICT(kind, hash) DO UPDATE SET
    origin = CASE WHEN excluded.origin = 'user_initiated' THEN 'user_initiated' ELSE asset_requests.origin END;

-- name: get_asset_requests?
-- param: kind: &str
SELECT hash, origin FROM asset_requests WHERE kind=:kind ORDER BY created_at, hash;

-- name: complete_asset_request!
-- param: kind: &str
-- param: hash: &str
DELETE FROM asset_requests WHERE kind=:kind AND hash=:hash;

-- name: add_book_download_request!
-- param: book_id: &str
-- param: origin: &str
-- param: created_at: i64
INSERT INTO book_download_requests(book_id, origin, created_at)
VALUES (:book_id, :origin, :created_at)
ON CONFLICT(book_id) DO UPDATE SET
    origin = CASE WHEN excluded.origin = 'user_initiated' THEN 'user_initiated' ELSE book_download_requests.origin END;

-- name: get_book_download_requests?
SELECT book_id, origin FROM book_download_requests ORDER BY created_at, book_id;

-- name: complete_book_download_request!
-- param: book_id: &str
DELETE FROM book_download_requests WHERE book_id=:book_id;

-- name: get_blob_any_path?
-- param: kind: &str
-- param: hash: &str
SELECT path
FROM blob_local_ref
WHERE kind = :kind
  AND hash = :hash
ORDER BY exists_flag DESC, last_seen DESC, path ASC
LIMIT 1;

-- name: get_book_paths_by_content_hash?
-- param: content_hash: &str
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
    CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END AS file_path
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
WHERE bd.content_hash = :content_hash
  AND bd.deleted_at IS NULL
ORDER BY file_path ASC;

-- Durable filesystem-operation outbox
-- name: enqueue_book_file_removal!
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
-- param: rel_path: &str
-- param: created_at: i64
INSERT OR IGNORE INTO pending_fs_operations(kind, dir_id, book_id, file_name, rel_path, created_at)
VALUES ('book_file_remove', :dir_id, :book_id, :file_name, :rel_path, :created_at);

-- name: cancel_book_file_removal!
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
DELETE FROM pending_fs_operations
WHERE kind = 'book_file_remove'
  AND dir_id = :dir_id
  AND book_id = :book_id
  AND file_name = :file_name;

-- name: enqueue_dir_removal!
-- param: dir_id: &str
-- param: rel_path: &str
-- param: created_at: i64
INSERT OR IGNORE INTO pending_fs_operations(kind, dir_id, rel_path, created_at)
VALUES ('dir_remove', :dir_id, :rel_path, :created_at);

-- name: cancel_dir_removal!
-- param: dir_id: &str
DELETE FROM pending_fs_operations
WHERE kind = 'dir_remove' AND dir_id = :dir_id;

-- name: get_pending_fs_operations?
SELECT id, kind, dir_id, book_id, file_name, rel_path
FROM pending_fs_operations
ORDER BY CASE kind WHEN 'book_file_remove' THEN 0 ELSE 1 END, id;

-- name: delete_pending_fs_operation!
-- param: id: i64
DELETE FROM pending_fs_operations WHERE id = :id;

-- name: fail_pending_fs_operation!
-- param: id: i64
-- param: last_error: &str
UPDATE pending_fs_operations
SET attempts = attempts + 1,
    last_error = :last_error
WHERE id = :id;

-- name: get_book_dir_removal_state?
-- param: dir_id: &str
-- param: book_id: &str
-- param: file_name: &str
SELECT deleted_at IS NOT NULL, content_hash
FROM book_dir
WHERE dir_id = :dir_id AND book_id = :book_id AND file_name = :file_name;

-- name: is_dir_live?
-- param: dir_id: &str
SELECT deleted_at IS NULL FROM dir WHERE id = :dir_id;

-- name: is_book_file_pending_removal?
-- param: dir_id: &str
-- param: file_name: &str
SELECT EXISTS(
    SELECT 1 FROM pending_fs_operations
    WHERE kind = 'book_file_remove'
      AND dir_id = :dir_id
      AND file_name = :file_name
);
