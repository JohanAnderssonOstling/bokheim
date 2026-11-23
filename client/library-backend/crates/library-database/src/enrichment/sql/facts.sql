-- File facts sent to the metadata server, and its answers.

-- name: facts_metadata_body?
-- param: content_hash: &BookId
SELECT body FROM sync_state_version WHERE state_kind='book_add' AND state_key=:content_hash AND state_subkey='' AND body IS NOT NULL;

-- name: facts_filenames_batch?
-- param: hashes: &str
WITH filenames AS (
    SELECT requested.key AS candidate_index, placement.file_name
    FROM json_each(:hashes) requested
    JOIN book b ON b.content_hash = unhex(requested.value)
    JOIN book_dir placement ON placement.book_row_id = b.row_id AND placement.deleted_at IS NULL
    GROUP BY requested.key, placement.file_name
), ranked AS (
    SELECT candidate_index, file_name,
           ROW_NUMBER() OVER (PARTITION BY candidate_index ORDER BY file_name) AS filename_rank
    FROM filenames
)
SELECT candidate_index, file_name FROM ranked
WHERE filename_rank <= 8
ORDER BY candidate_index, filename_rank;

-- name: facts_candidates?
-- param: now: i64
-- param: after: Option<&BookId>
SELECT b.content_hash, b.format, record.body
FROM book b
JOIN sync_state_version record ON record.state_key=b.content_hash AND record.state_kind='book_add' AND record.state_subkey='' AND record.body IS NOT NULL
JOIN book_transfer transfer ON transfer.content_hash=b.content_hash
WHERE b.deleted_at IS NULL AND transfer.enrichment_retry_at <= :now AND b.content_hash>COALESCE(:after,x'')
ORDER BY b.content_hash LIMIT 256;

-- name: facts_next_retry ->
SELECT MIN(transfer.enrichment_retry_at) FROM book_transfer transfer JOIN book b USING(content_hash) WHERE b.deleted_at IS NULL;

-- name: facts_cover_candidates?
-- param: after: Option<&BookId>
SELECT b.content_hash
FROM book b
JOIN book_transfer t ON t.content_hash = b.content_hash
WHERE t.cover_checked=0 AND t.cover_state='no_cover' AND b.deleted_at IS NULL
  AND b.content_hash>COALESCE(:after,x'')
  AND EXISTS(SELECT 1 FROM sync_state_version s WHERE s.state_key=b.content_hash AND s.state_subkey='' AND s.state_kind='book_add' AND s.body IS NOT NULL)
ORDER BY b.content_hash LIMIT 256;

-- name: facts_mark_cover_checked!
-- param: content_hash: &BookId
INSERT INTO book_transfer(content_hash,cover_checked) SELECT content_hash,1 FROM book WHERE content_hash=:content_hash
ON CONFLICT(content_hash) DO UPDATE SET cover_checked=1;

-- name: facts_cover_input ->
-- param: hash: &BookId
SELECT record.body,transfer.cover_state FROM book b
JOIN book_transfer transfer ON transfer.content_hash=b.content_hash
LEFT JOIN sync_state_version record ON record.state_key=b.content_hash AND record.state_kind='book_add' AND record.state_subkey=''
WHERE b.content_hash=:hash AND b.deleted_at IS NULL;

-- name: facts_mark_cover_checked_if_current!
-- param: hash: &BookId
-- param: body: Option<&[u8]>
INSERT INTO book_transfer(content_hash,cover_checked)
SELECT b.content_hash,1 FROM book b
LEFT JOIN sync_state_version record ON record.state_key=b.content_hash AND record.state_kind='book_add' AND record.state_subkey=''
WHERE b.content_hash=:hash AND b.deleted_at IS NULL AND record.body IS :body
ON CONFLICT(content_hash) DO UPDATE SET cover_checked=1 WHERE book_transfer.cover_checked=0;
