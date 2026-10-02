-- File facts sent to the metadata server, and its answers.

-- name: facts_metadata_body?
-- param: content_hash: &str
SELECT body FROM sync_state_version WHERE state_kind = 'metadata' AND state_key = :content_hash AND body IS NOT NULL;

-- name: facts_filenames?
-- param: content_hash: &str
SELECT DISTINCT file_name FROM book_dir
WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash = :content_hash) AND deleted_at IS NULL
ORDER BY file_name LIMIT 8;

-- name: facts_candidates?
-- param: scope: &str
SELECT b.content_hash, b.format, e.request_key, e.status, e.retry_at
FROM json_each(:scope) member
JOIN book b ON b.content_hash = member.value
LEFT JOIN book_enrichment e ON e.content_hash = b.content_hash
WHERE b.deleted_at IS NULL
ORDER BY b.content_hash;

-- name: facts_answer?
-- param: content_hash: &str
SELECT request_key, response FROM book_enrichment WHERE content_hash = :content_hash AND status = 'found';

-- name: facts_record!
-- param: content_hash: &str
-- param: request_key: &str
-- param: status: &str
-- param: response: Option<&str>
-- param: retry_at: i64
INSERT INTO book_enrichment(content_hash, request_key, status, response, retry_at)
SELECT content_hash, :request_key, :status, :response, :retry_at FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET request_key = excluded.request_key, status = excluded.status,
    response = excluded.response, retry_at = excluded.retry_at, cover_checked = 0;

-- name: facts_record_failure!
-- param: content_hash: &str
-- param: request_key: &str
-- param: retry_at: i64
-- An earlier answer stays until a lookup for the new facts succeeds.
INSERT INTO book_enrichment(content_hash, request_key, status, response, retry_at)
SELECT content_hash, :request_key, 'failed', NULL, :retry_at FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET retry_at = excluded.retry_at,
    status = CASE WHEN book_enrichment.status = 'found' THEN 'found' ELSE 'failed' END;

-- name: facts_mark_dirty!
-- param: content_hash: &str
INSERT OR IGNORE INTO sync_projection_dirty VALUES('book_projection', :content_hash, '');

-- name: facts_next_retry ->
SELECT MIN(e.retry_at) FROM book_enrichment e JOIN book b USING(content_hash) WHERE b.deleted_at IS NULL;

-- name: facts_cover_candidates?
-- param: scope: &str
SELECT e.content_hash
FROM json_each(:scope) member
JOIN book_enrichment e ON e.content_hash = member.value
JOIN book_transfer t ON t.content_hash = e.content_hash
JOIN book b ON b.content_hash = e.content_hash
WHERE e.status = 'found' AND NOT e.cover_checked AND t.cover_state = 'no_cover' AND b.deleted_at IS NULL
ORDER BY e.content_hash;

-- name: facts_mark_cover_checked!
-- param: content_hash: &str
UPDATE book_enrichment SET cover_checked = 1 WHERE content_hash = :content_hash;
