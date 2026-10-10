-- File facts sent to the metadata server, and its answers.

-- name: facts_metadata_body?
-- param: content_hash: &ContentHash
SELECT body FROM local_book_draft WHERE content_hash=:content_hash;

-- name: facts_filenames?
-- param: content_hash: &ContentHash
SELECT DISTINCT file_name FROM book_dir
WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash = :content_hash) AND deleted_at IS NULL
ORDER BY file_name LIMIT 8;

-- name: facts_candidates?
-- param: scope: &str
SELECT b.content_hash, b.format, draft.retry_at
FROM json_each(:scope) member
JOIN book b ON b.content_hash = unhex(member.value)
JOIN local_book_draft draft ON draft.content_hash = b.content_hash
WHERE b.deleted_at IS NULL
ORDER BY b.content_hash;

-- name: facts_record_failure!
-- param: content_hash: &ContentHash
-- param: retry_at: i64
UPDATE local_book_draft SET retry_at=:retry_at WHERE content_hash=:content_hash;

-- name: facts_next_retry ->
SELECT MIN(draft.retry_at) FROM local_book_draft draft JOIN book b USING(content_hash) WHERE b.deleted_at IS NULL;

-- name: facts_cover_candidates?
-- param: scope: &str
SELECT b.content_hash
FROM json_each(:scope) member
JOIN book b ON b.content_hash = unhex(member.value)
JOIN book_transfer t ON t.content_hash = b.content_hash
WHERE t.cover_checked=0 AND t.cover_state='no_cover' AND b.deleted_at IS NULL
  AND EXISTS(SELECT 1 FROM sync_book_record s WHERE s.content_hash=b.content_hash AND s.state_kind='book_add')
ORDER BY b.content_hash;

-- name: facts_mark_cover_checked!
-- param: content_hash: &ContentHash
INSERT INTO book_transfer(content_hash,cover_checked) SELECT content_hash,1 FROM book WHERE content_hash=:content_hash
ON CONFLICT(content_hash) DO UPDATE SET cover_checked=1;
