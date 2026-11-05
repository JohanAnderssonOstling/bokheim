-- The one synchronized reading-position register, shared by every format.

-- name: reading_state?
-- param: content_hash: &BookId
-- Hidden books expose no resume position, but retain progress for edits.
SELECT CASE WHEN deleted_at IS NULL THEN read_pos END, read_progress
FROM book WHERE content_hash = :content_hash;

-- name: reading_upsert_toc_entry!
-- param: content_hash: &BookId
-- param: target: &str
INSERT INTO book_local_state(content_hash,reading_target)
VALUES(:content_hash,:target)
ON CONFLICT(content_hash) DO UPDATE SET reading_target=excluded.reading_target;
