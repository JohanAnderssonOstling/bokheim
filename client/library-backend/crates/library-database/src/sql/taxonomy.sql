-- Read and rebuild local taxonomy assignments in bounded book batches.

-- name: taxonomy_content_revision ->
SELECT value FROM curated.taxonomy_meta WHERE key='client_revision';

-- name: taxonomy_stored_revision ->
SELECT taxonomy_revision FROM library_local_state WHERE singleton=1;

-- name: taxonomy_book_page ?
-- param: row_id: &SqlParam<'_>
SELECT row_id FROM book WHERE row_id>:row_id ORDER BY row_id LIMIT 128;

-- name: taxonomy_book_subjects ?
-- param: book_row_id: &SqlParam<'_>
SELECT name,source,authority,code FROM book_subject WHERE book_row_id=:book_row_id ORDER BY position;

-- name: taxonomy_set_revision !
-- param: taxonomy_revision: &SqlParam<'_>
UPDATE library_local_state SET taxonomy_revision=:taxonomy_revision WHERE singleton=1;
