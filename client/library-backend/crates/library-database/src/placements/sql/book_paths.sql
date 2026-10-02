-- Local book-path resolution and cached PDF metadata.

-- name: stored_pdf_metadata?
-- param: content_hash: &str
-- param: checksum: &str
SELECT metadata
FROM pdf_reader_metadata
WHERE content_hash = :content_hash AND checksum = :checksum;

-- name: readable_placements?
-- param: content_hash: &str
-- A book can have more than one live placement (filed in two folders, or a
-- remote mutation adding a second one); only some may have local bytes.
-- Placements survive Trash, so a trashed book is still readable.
SELECT bd.dir_id, bd.file_name, bd.is_downloaded
FROM book_dir bd
WHERE bd.book_row_id = (SELECT row_id FROM book WHERE content_hash = :content_hash)
  AND bd.deleted_at IS NULL;

-- name: book_projection_paths?
-- param: content_hash: &str
-- Where the book's bytes were last materialized on disk.
SELECT relative_path FROM local_file_projection WHERE content_hash = :content_hash;

-- name: local_book_projection_paths?
SELECT p.relative_path
FROM local_file_projection p
JOIN book b ON b.content_hash = p.content_hash
WHERE b.deleted_at IS NULL;

-- name: local_book_placements?
SELECT bd.dir_id, bd.file_name
FROM book_dir bd
JOIN book b ON b.row_id = bd.book_row_id
WHERE b.deleted_at IS NULL AND bd.deleted_at IS NULL;

-- name: local_pdf_checksum ->
-- param: content_hash: &str
-- param: dir_id: &str
-- param: file_name: &str
SELECT bd.checksum
FROM book b
JOIN book_dir bd ON bd.book_row_id = b.row_id
WHERE b.content_hash = :content_hash
  AND bd.dir_id = :dir_id
  AND bd.file_name = :file_name
  AND bd.checksum IS NOT NULL
  AND bd.deleted_at IS NULL
  AND b.deleted_at IS NULL;

