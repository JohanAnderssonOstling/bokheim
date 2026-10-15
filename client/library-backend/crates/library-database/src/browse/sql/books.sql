-- name: all_book_ids?
-- Every visible book once, regardless of how many placements it has.
SELECT book.row_id FROM book
WHERE book.deleted_at IS NULL
  AND EXISTS(SELECT 1 FROM library_book_presence presence
             WHERE presence.book_row_id=book.row_id);

-- name: library_book_hashes?
-- Every visible book's hash, to mark catalog books already in the library.
SELECT book.content_hash FROM book
WHERE book.deleted_at IS NULL
  AND EXISTS(SELECT 1 FROM library_book_presence presence
             WHERE presence.book_row_id=book.row_id);

-- name: matching_books?
-- param: book_ids: &str
-- param: downloaded_only: i32
-- param: file_types: i32
-- param: languages: &str
-- param: normalized_query: &str
-- param: load_popularity: i32
SELECT book.row_id,
       COALESCE((SELECT downloaded FROM library_book_presence WHERE book_row_id=book.row_id),0),
       instr(book.search_title,:normalized_query)>0 OR instr(book.search_author,:normalized_query)>0,
       book.read_progress,
       CASE WHEN :load_popularity=1 THEN book.popularity_rating_count END
FROM json_each(:book_ids) selected
JOIN book ON book.row_id=selected.value
WHERE book.deleted_at IS NULL
  AND EXISTS(SELECT 1 FROM library_book_presence presence
             WHERE presence.book_row_id=book.row_id AND (:downloaded_only=0 OR presence.downloaded=1))
  AND (:file_types=0 OR (:file_types & book.format)!=0)
  AND (:languages='' OR EXISTS(SELECT 1 FROM book_language language
       WHERE language.book_row_id=book.row_id AND instr(:languages,',' || language.language || ',')>0));

-- name: ordered_books?
-- param: book_ids: &str
-- param: book_sort: i32
SELECT book.row_id
FROM json_each(:book_ids) selected
JOIN book ON book.row_id=selected.value
ORDER BY CASE WHEN :book_sort=1 THEN book.read_at END DESC,
         CASE WHEN :book_sort=2 THEN book.popularity_rating_count IS NULL END,
         CASE WHEN :book_sort=2 THEN book.popularity_rating_count END DESC,
         book.search_title,book.title,book.content_hash;
