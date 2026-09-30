-- name: count_all_books?
-- Counts only books with a live placement, matching what the carousels display.
SELECT COUNT(*) FROM book
WHERE deleted_at IS NULL
  AND EXISTS (SELECT 1 FROM book_dir bd WHERE bd.book_row_id = book.row_id AND bd.deleted_at IS NULL);

-- name: library_facets?
-- Facet choices describe the whole visible library, independent of the page
-- location and its active filters.
WITH eligible AS MATERIALIZED (
    SELECT book.row_id, book.format
    FROM book
    WHERE book.deleted_at IS NULL
      AND EXISTS(SELECT 1 FROM book_dir placement
                 WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL)
)
SELECT 0 AS kind, format AS value, COUNT(*) AS book_count
FROM eligible GROUP BY format
UNION ALL
SELECT 1, language.language, COUNT(*)
FROM eligible JOIN book_language language ON language.book_row_id=eligible.row_id
GROUP BY language.language
ORDER BY kind,value;
