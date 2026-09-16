-- name: get_book_file_path?
-- param: content_hash: &str
WITH RECURSIVE ancestors(id,parent_id,path) AS (
    SELECT d.id,d.parent_id,
        CASE WHEN d.id='00000000-0000-0000-0000-000000000000' THEN bd.file_name ELSE d.name || '/' || bd.file_name END
    FROM book_dir bd JOIN dir d ON d.id=bd.dir_id
    WHERE bd.book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash)
      AND bd.deleted_at IS NULL AND d.deleted_at IS NULL
    UNION ALL
    SELECT d.id,d.parent_id,
        CASE WHEN d.id='00000000-0000-0000-0000-000000000000' THEN a.path ELSE d.name || '/' || a.path END
    FROM ancestors a JOIN dir d ON d.id=a.parent_id
    WHERE a.id!=a.parent_id AND d.deleted_at IS NULL
)
SELECT '/' || path FROM ancestors
WHERE id='00000000-0000-0000-0000-000000000000'
ORDER BY path LIMIT 1;

-- name: is_book_downloaded?
-- param: content_hash: &str
SELECT COUNT(*) > 0
FROM book_dir
WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
  AND deleted_at is null
  AND is_downloaded = 1;

-- name: is_book_download_requested?
-- param: content_hash: &str
SELECT
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

