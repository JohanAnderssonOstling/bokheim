-- name: matching_books?
-- param: book_ids: &str
-- param: downloaded_only: i32
-- param: file_types: i32
-- param: languages: &str
-- param: normalized_query: &str
SELECT book.row_id,
       EXISTS(SELECT 1 FROM book_dir placement
              WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL
                AND placement.is_downloaded=1),
       instr(book.search_title,:normalized_query)>0 OR instr(book.search_author,:normalized_query)>0,
       book.read_progress
FROM json_each(:book_ids) selected
JOIN book ON book.row_id=selected.value
WHERE book.deleted_at IS NULL
  AND EXISTS(SELECT 1 FROM book_dir placement
             WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL
               AND (:downloaded_only=0 OR placement.is_downloaded=1))
  AND (:file_types=0 OR (:file_types & book.format)!=0)
  AND (:languages='' OR EXISTS(SELECT 1 FROM book_language language
       WHERE language.book_row_id=book.row_id AND instr(:languages,',' || language.language || ',')>0));

-- name: ordered_books?
-- param: book_ids: &str
-- param: book_sort: i32
SELECT book.row_id
FROM json_each(:book_ids) selected
JOIN book ON book.row_id=selected.value
ORDER BY CASE WHEN :book_sort=1 THEN book.read_at END DESC,book.search_title,book.title,book.content_hash;
