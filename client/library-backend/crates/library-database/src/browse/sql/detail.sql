-- name: book_detail_book?
-- param: content_hash: &ContentHash
-- The book's id when it is visible (not deleted or hidden, with a live
-- placement), its audiobook chapter count, and its addition record, which
-- holds the publication year.
SELECT book.row_id,
       CASE WHEN book.duration_ms IS NOT NULL THEN book.entry_count END AS audiobook_chapter_count,
       (SELECT body FROM sync_book_record record
        WHERE record.state_kind='book_add' AND record.content_hash=book.content_hash)
FROM book
WHERE book.content_hash=:content_hash AND book.deleted_at IS NULL
  AND EXISTS(SELECT 1 FROM library_book_presence presence
             WHERE presence.book_row_id=book.row_id);

-- name: book_detail_description?
-- param: content_hash: &ContentHash
SELECT description FROM book_description WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: book_detail_directory?
-- param: content_hash: &ContentHash
SELECT dir_id FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND deleted_at IS NULL ORDER BY dir_id LIMIT 1;

-- name: book_detail_authors?
-- param: content_hash: &ContentHash
-- param: author_role: Vec<u8>
SELECT identity.stable_id,credit.name FROM book_contributor credit JOIN author_identity identity ON identity.id=credit.author_identity_id WHERE credit.role=:author_role AND credit.book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) ORDER BY credit.position;

-- name: book_detail_credits?
-- param: content_hash: &ContentHash
-- param: narrator_role: Vec<u8>
SELECT identity.stable_id,contributor.name,contributor.role FROM book_contributor contributor JOIN author_identity identity ON identity.id=contributor.author_identity_id WHERE contributor.book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND contributor.role=:narrator_role ORDER BY contributor.position;
