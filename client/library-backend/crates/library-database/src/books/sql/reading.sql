-- The one synchronized reading-position register, shared by every format.

-- name: reading_get_position?
-- param: content_hash: &ContentHash
SELECT read_pos FROM book WHERE content_hash = :content_hash AND deleted_at IS NULL;

-- name: reading_upsert_toc_entry!
-- param: content_hash: &ContentHash
-- param: target: &str
INSERT INTO book_local_state(content_hash,reading_target)
VALUES(:content_hash,:target)
ON CONFLICT(content_hash) DO UPDATE SET reading_target=excluded.reading_target;


-- name: reading_progress ->
-- param: content_hash: &SqlParam<'_>
SELECT read_progress FROM book WHERE content_hash=:content_hash;
