-- Library-facing read models for paths, folders, book grids, and authors.

-- name: live_directory_exists?
-- param: directory_id: &str
SELECT EXISTS(
    SELECT 1 FROM dir WHERE id=:directory_id AND deleted_at IS NULL
);

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
    book.content_hash,
    CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END AS file_path,
    bd.is_downloaded
FROM book_dir bd
JOIN dir_paths ON bd.dir_id = dir_paths.id
JOIN book ON book.row_id = bd.book_row_id
WHERE bd.deleted_at IS NULL AND book.content_hash != '';

-- name: get_book_paths_without_title?
SELECT
    b.content_hash,
    CASE
        WHEN dp.path = ''
            THEN bd.file_name
        ELSE dp.path || '/' || bd.file_name
    END AS full_path
FROM book b
JOIN book_dir bd ON bd.book_row_id = b.row_id
JOIN dir_paths dp ON dp.id = bd.dir_id
WHERE b.deleted_at IS NULL
  AND (b.title IS NULL OR NOT EXISTS (SELECT 1 FROM author_credit WHERE book_row_id = b.row_id))
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
    COUNT(DISTINCT book.row_id) AS book_count,
    COUNT(DISTINCT CASE WHEN placement.is_downloaded = 1 THEN book.row_id END) AS downloaded_book_count
FROM dir AS root
JOIN descendants ON descendants.root_id = root.id
LEFT JOIN book_dir AS placement ON placement.dir_id = descendants.id AND placement.deleted_at IS NULL
LEFT JOIN book ON book.row_id = placement.book_row_id AND book.deleted_at IS NULL AND book.hidden_at IS NULL
WHERE root.parent_id = :dir_id
  AND root.id != :dir_id
  AND root.deleted_at IS NULL
GROUP BY root.id, root.name;

-- Card-shaped reads share the book_cards view; each query only owns scope and ordering.

-- name: search_book_cards?
-- param: query: &str
-- param: downloaded_only: i32
-- param: limit: i64
SELECT content_hash, title, author, description, progress, downloaded, download_requested,
       audiobook, audiobook_duration_ms, audiobook_chapter_count
FROM book_cards
WHERE instr(LOWER(COALESCE(title, '')), :query) > 0
  AND (:downloaded_only = 0 OR downloaded)
ORDER BY LOWER(COALESCE(title, '')), content_hash
LIMIT :limit;

-- name: get_book_cards_in_dir?
-- param: dir_id: &str
-- param: downloaded_only: i32
SELECT DISTINCT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.audiobook,
       card.audiobook_duration_ms, card.audiobook_chapter_count
FROM book_cards card
JOIN book_dir placement ON placement.book_row_id = card.book_row_id
WHERE placement.dir_id = :dir_id AND placement.deleted_at IS NULL
  AND (:downloaded_only = 0 OR card.downloaded)
ORDER BY LOWER(COALESCE(card.title, '')), card.content_hash;

-- name: get_book_cards_by_author?
-- param: stable_id: &str
-- param: downloaded_only: i32
SELECT card.content_hash, card.title, card.author, card.description, card.progress,
       card.downloaded, card.download_requested, card.audiobook,
       card.audiobook_duration_ms, card.audiobook_chapter_count
FROM book_cards card
JOIN author_credit credit ON credit.book_row_id = card.book_row_id
JOIN author_identity identity ON identity.id = credit.author_identity_id
WHERE identity.stable_id = :stable_id
  AND (:downloaded_only = 0 OR card.downloaded)
ORDER BY LOWER(COALESCE(card.title, '')), card.content_hash;

-- name: get_descendant_content_hashes?
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
SELECT DISTINCT book.content_hash
FROM book_dir placement
JOIN subtree ON subtree.id = placement.dir_id
JOIN book ON book.row_id = placement.book_row_id
WHERE placement.deleted_at IS NULL
  AND book.deleted_at IS NULL
  AND book.hidden_at IS NULL
ORDER BY book.content_hash;

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
    COUNT(DISTINCT placement.book_row_id),
    COUNT(DISTINCT CASE WHEN placement.is_downloaded = 1 THEN placement.book_row_id END)
FROM book_dir placement
JOIN subtree ON subtree.id = placement.dir_id
JOIN book ON book.row_id = placement.book_row_id
WHERE placement.deleted_at IS NULL
  AND book.deleted_at IS NULL
  AND book.hidden_at IS NULL;

-- name: get_book_ancestor_dir_ids?
-- param: content_hash: &str
WITH RECURSIVE ancestors(id) AS (
    SELECT placement.dir_id
    FROM book_dir placement
    WHERE placement.book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
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
-- param: downloaded_only: i32
SELECT
    author_identity.stable_id AS author_id,
    MIN(author_credit.credited_name) AS display_name,
    COUNT(DISTINCT book.row_id) AS book_count
FROM author_identity
JOIN author_credit ON author_credit.author_identity_id = author_identity.id
JOIN book ON book.row_id = author_credit.book_row_id
JOIN book_dir ON book_dir.book_row_id = book.row_id
WHERE book.deleted_at IS NULL
  AND book.hidden_at IS NULL
  AND book_dir.deleted_at IS NULL
  AND TRIM(author_credit.credited_name) != ''
  AND (:downloaded_only = 0 OR EXISTS (
      SELECT 1 FROM book_dir available
      WHERE available.book_row_id = book.row_id
        AND available.deleted_at IS NULL
        AND available.is_downloaded = 1
  ))
GROUP BY author_identity.id, author_identity.stable_id
ORDER BY LOWER(display_name);

-- name: get_author_discovery_seed?
-- param: stable_id: &str
SELECT
    MIN(author_credit.credited_name) AS display_name,
    book_identifier.scheme,
    COALESCE(book_identifier.canonical_value, book_identifier.value) AS identifier_value
FROM author_identity
JOIN author_credit ON author_credit.author_identity_id = author_identity.id
JOIN book ON book.row_id = author_credit.book_row_id
JOIN book_dir ON book_dir.book_row_id = book.row_id
LEFT JOIN book_identifier ON book_identifier.book_row_id = book.row_id
WHERE author_identity.stable_id = :stable_id
  AND book.deleted_at IS NULL
  AND book.hidden_at IS NULL
  AND book_dir.deleted_at IS NULL
  AND TRIM(author_credit.credited_name) != ''
GROUP BY book_identifier.scheme, COALESCE(book_identifier.canonical_value, book_identifier.value)
ORDER BY book_identifier.scheme, identifier_value;
