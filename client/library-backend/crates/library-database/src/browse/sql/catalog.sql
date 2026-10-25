-- name: catalog_browse_matching?
-- param: book_ids: &str
-- param: held_hashes: &str
-- param: file_types: i32
-- param: languages: &str
-- param: normalized_query: &str
SELECT book.row_id,
       CASE WHEN :normalized_query='' THEN 1
         ELSE instr(book.search_title,:normalized_query)>0 OR instr(book.search_author,:normalized_query)>0 END,
       book.source_priority=0,book.source_priority,book.popularity_score,NULL
FROM json_each(:book_ids) selected
JOIN catalog_browse_book book ON book.row_id=selected.value
WHERE (:file_types=0 OR (:file_types & book.format)!=0)
  AND (:languages='' OR EXISTS(SELECT 1 FROM catalog_browse_language language
       WHERE language.book_row_id=book.row_id AND instr(:languages,',' || language.language || ',')>0))
  AND (book.external=1 OR lower(hex(book.key)) NOT IN (SELECT value FROM json_each(:held_hashes)));

-- name: catalog_browse_cards?
-- param: book_ids: &str
-- param: sort_popularity: i32
SELECT book.row_id,book.key,CASE WHEN book.external=0 THEN book.key END,
       source.title,source.subtitle,
       COALESCE((SELECT group_concat(name,', ') FROM (
           SELECT author.name FROM catalog_browse_author_book credit JOIN catalog_browse_author author USING(author_id)
           WHERE credit.book_row_id=book.row_id ORDER BY author.name,author.author_id)),''),book.format,
       book.external=1 AND book.format=16 AND source.download_url IS NOT NULL,book.duration_ms
FROM json_each(:book_ids) selected
JOIN catalog_browse_book book ON book.row_id=selected.value
JOIN book source ON source.row_id=book.source_row_id
ORDER BY (book.source_priority=0) DESC,
         CASE WHEN :sort_popularity=1 THEN book.source_priority END,
         CASE WHEN :sort_popularity=1 THEN book.popularity_score IS NULL END,
         CASE WHEN :sort_popularity=1 THEN book.popularity_score END DESC,
         book.search_title,source.title,book.key;

-- name: catalog_browse_ordered?
-- param: book_ids: &str
-- param: sort_popularity: i32
-- Choose each section's page before assembling author strings and card rows.
SELECT book.row_id
FROM json_each(:book_ids) selected
JOIN catalog_browse_book book ON book.row_id=selected.value
JOIN book source ON source.row_id=book.source_row_id
ORDER BY (book.source_priority=0) DESC,
         CASE WHEN :sort_popularity=1 THEN book.source_priority END,
         CASE WHEN :sort_popularity=1 THEN book.popularity_score IS NULL END,
         CASE WHEN :sort_popularity=1 THEN book.popularity_score END DESC,
         book.search_title,source.title,book.key;

-- name: catalog_browse_authors?
SELECT author.author_id,author.name,credit.book_row_id
FROM catalog_browse_author author JOIN catalog_browse_author_book credit USING(author_id);

-- name: catalog_browse_languages?
SELECT language,count(*) FROM catalog_browse_language GROUP BY language ORDER BY language;

-- name: catalog_browse_formats?
SELECT format,count(*) FROM catalog_browse_book GROUP BY format;

-- name: catalog_browse_concepts?
SELECT book_row_id,concept_id FROM catalog_browse_concept;
