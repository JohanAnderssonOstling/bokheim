-- name: schema_placements_views &
-- Shared library eligibility. Membership and local availability are independent
-- of browse location; folder browsing additionally scopes these facts to a folder.
-- Keep the existing semantics: a live membership qualifies even when its folder
-- is suppressed. Folder ancestry visibility remains the directory projection's job.
CREATE VIEW IF NOT EXISTS library_book_presence AS
SELECT book.row_id AS book_row_id,
       EXISTS(SELECT 1 FROM book_dir placement
              WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL
                AND placement.is_downloaded=1)
       OR EXISTS(SELECT 1 FROM local_file_projection source WHERE source.content_hash=book.content_hash) AS downloaded
FROM book
WHERE book.deleted_at IS NULL
  AND EXISTS(SELECT 1 FROM book_dir placement
             WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL);
