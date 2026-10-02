-- Thumbnail jobs and upload preparation.

-- name: thumbnail_pending_job_count ->
SELECT COUNT(*) FROM book_transfer w JOIN book b ON b.content_hash=w.content_hash WHERE w.cover_state = 'pending' AND b.deleted_at IS NULL;

-- name: thumbnail_pending_jobs?
-- param: after: &str
SELECT w.content_hash, w.cover_retry_after FROM book_transfer w
JOIN book b ON b.content_hash = w.content_hash AND b.deleted_at IS NULL
WHERE w.cover_state = 'pending' AND w.content_hash > :after
ORDER BY w.content_hash LIMIT 32;

-- name: thumbnail_local_format?
-- param: content_hash: &str
SELECT b.format
FROM book b
WHERE b.content_hash = :content_hash
  AND EXISTS(
    SELECT 1 FROM book_dir bd JOIN dir d ON d.id = bd.dir_id
    WHERE bd.book_row_id = b.row_id AND bd.deleted_at IS NULL AND d.deleted_at IS NULL
  );

-- name: thumbnail_has_remote ->
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book_transfer WHERE content_hash = :content_hash AND server_has_cover);

-- name: thumbnail_download_coverage ->
-- Count known cloud thumbnails for visible books and those already ready here.
SELECT COUNT(CASE WHEN transfer.cover_state = 'ready' THEN 1 END), COUNT(*)
FROM book_transfer transfer
JOIN book ON book.content_hash = transfer.content_hash
WHERE transfer.server_has_cover AND book.deleted_at IS NULL;

-- name: thumbnail_remote_presence_candidates?
-- param: after: &str
SELECT DISTINCT b.content_hash FROM book b
JOIN book_dir bd ON bd.book_row_id = b.row_id AND bd.deleted_at IS NULL
JOIN dir d ON d.id = bd.dir_id AND d.deleted_at IS NULL
WHERE b.content_hash > :after AND b.deleted_at IS NULL
ORDER BY b.content_hash LIMIT 512;

-- name: thumbnail_set_result!
-- param: content_hash: &str
-- param: state: &str
INSERT INTO book_transfer(content_hash, cover_state) SELECT content_hash, :state FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET cover_state = excluded.cover_state, cover_retry_after = 0;

-- name: thumbnail_retry!
-- param: content_hash: &str
-- param: retry_after: i64
INSERT INTO book_transfer(content_hash, cover_state, cover_retry_after)
SELECT content_hash, 'pending', :retry_after FROM book WHERE content_hash = :content_hash AND deleted_at IS NULL
ON CONFLICT(content_hash) DO UPDATE SET cover_state = 'pending', cover_retry_after = excluded.cover_retry_after;

-- name: thumbnail_discard!
-- param: content_hash: &str
UPDATE book_transfer SET cover_state = NULL, cover_retry_after = 0 WHERE content_hash = :content_hash;

-- name: thumbnail_request_missing!
-- param: content_hash: &str
INSERT INTO book_transfer(content_hash, cover_state)
SELECT content_hash, 'pending' FROM book WHERE content_hash = :content_hash AND deleted_at IS NULL
ON CONFLICT(content_hash) DO UPDATE SET cover_state = 'pending', cover_retry_after = 0
WHERE cover_state IS NULL OR cover_state = 'ready';

-- name: thumbnail_work_state?
-- param: content_hash: &str
SELECT cover_state, cover_retry_after FROM book_transfer WHERE content_hash = :content_hash AND cover_state IS NOT NULL;

-- name: thumbnail_upload_allowed ->
-- param: content_hash: &str
-- param: rejected_after: i64
SELECT EXISTS(SELECT 1 FROM uploadable_covers
WHERE content_hash=:content_hash AND NOT COALESCE(cover_rejected_at > :rejected_after, 0));

-- name: upload_preparation_snapshot?
SELECT b.content_hash, u.upload_id
FROM book b LEFT JOIN book_transfer u ON u.content_hash = b.content_hash
WHERE b.deleted_at IS NULL
ORDER BY b.content_hash;

-- name: prepared_upload_batch?
-- param: scope: &str
-- param: affected_dirs: &str
-- `affected_dirs`: folders with queued physical work and everything below them.
SELECT b.content_hash, u.upload_id
FROM json_each(:scope) members JOIN book b ON b.content_hash = members.value
LEFT JOIN book_transfer u ON u.content_hash = b.content_hash
WHERE b.deleted_at IS NULL
  AND NOT EXISTS(SELECT 1 FROM local_book_work w WHERE w.content_hash = b.content_hash)
  AND NOT EXISTS(SELECT 1 FROM local_file_work w WHERE w.content_hash = b.content_hash AND w.operation IN ('materialize', 'remove'))
  AND NOT EXISTS(SELECT 1 FROM book_dir bd WHERE bd.book_row_id = b.row_id AND bd.dir_id IN (SELECT value FROM json_each(:affected_dirs)))
  AND NOT EXISTS(SELECT 1 FROM local_file_projection fp WHERE fp.content_hash = b.content_hash AND fp.dir_id IN (SELECT value FROM json_each(:affected_dirs)))
ORDER BY b.content_hash;

-- name: upload_thumbnail_candidates?
-- param: scope: &str
-- param: now: i64
SELECT w.content_hash
FROM json_each(:scope) members
JOIN book_transfer w ON w.content_hash = members.value
WHERE w.cover_state = 'pending' AND w.cover_retry_after <= :now
ORDER BY w.content_hash;

-- name: finish_upload_preparation!
-- param: members: &str
-- A book prepared with no upload records 0; upload ids start at 1.
INSERT INTO book_transfer(content_hash, upload_prepared)
SELECT b.content_hash, COALESCE(json_extract(member.value, '$[1]'), 0)
FROM json_each(:members) member
JOIN book b ON b.content_hash = json_extract(member.value, '$[0]')
WHERE b.deleted_at IS NULL
ON CONFLICT(content_hash) DO UPDATE SET upload_prepared = excluded.upload_prepared
WHERE upload_prepared IS NOT excluded.upload_prepared;

