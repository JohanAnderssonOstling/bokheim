-- name: get_book_reading_position?
-- param: content_hash: &str
SELECT read_pos FROM book WHERE content_hash = :content_hash AND deleted_at IS NULL;

-- name: set_reading_position!
-- param: content_hash: &str
-- param: position: &str
-- param: progress: Option<f32>
-- A reader which cannot cheaply calculate book-wide progress (PDF,
-- audiobook seeking) updates only the canonical location register.
UPDATE book
SET read_pos = :position,
    read_progress = COALESCE(:progress, read_progress)
WHERE content_hash = :content_hash;
