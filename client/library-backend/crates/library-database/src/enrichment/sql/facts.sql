-- File facts sent to the metadata server, and its answers.

-- name: facts_metadata_body?
-- param: content_hash: &BookId
SELECT body FROM local_book_draft WHERE content_hash=:content_hash;

-- name: facts_inputs_batch?
-- param: hashes: &str
SELECT unhex(requested.value),draft.body,record.body,record.publication
FROM json_each(:hashes) requested
LEFT JOIN local_book_draft draft ON draft.content_hash=unhex(requested.value)
LEFT JOIN sync_book_record record ON record.content_hash=unhex(requested.value) AND record.state_kind='book_add';

-- name: facts_record_failures_batch!
-- param: hashes: &str
-- param: retry_at: i64
UPDATE local_book_draft SET retry_at=:retry_at
WHERE content_hash IN (SELECT unhex(value) FROM json_each(:hashes));

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
-- param: scope: &str
-- param: now: i64
SELECT b.content_hash, b.format, draft.body
FROM json_each(:scope) member
JOIN book b ON b.content_hash = unhex(member.value)
JOIN local_book_draft draft ON draft.content_hash = b.content_hash
WHERE b.deleted_at IS NULL AND (draft.retry_at IS NULL OR draft.retry_at <= :now)
ORDER BY b.content_hash;

-- name: facts_record_failure!
-- param: content_hash: &BookId
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
-- param: content_hash: &BookId
INSERT INTO book_transfer(content_hash,cover_checked) SELECT content_hash,1 FROM book WHERE content_hash=:content_hash
ON CONFLICT(content_hash) DO UPDATE SET cover_checked=1;

-- name: facts_cover_inputs_batch?
-- param: hashes: &str
SELECT unhex(requested.value),record.body,record.publication,transfer.cover_state
FROM json_each(:hashes) requested
LEFT JOIN sync_book_record record ON record.content_hash=unhex(requested.value) AND record.state_kind='book_add'
LEFT JOIN book_transfer transfer ON transfer.content_hash=unhex(requested.value);

-- name: facts_mark_covers_checked_batch!
-- param: hashes: &str
INSERT INTO book_transfer(content_hash,cover_checked)
SELECT b.content_hash,1 FROM json_each(:hashes) requested
JOIN book b ON b.content_hash=unhex(requested.value)
WHERE true
ON CONFLICT(content_hash) DO UPDATE SET cover_checked=1 WHERE book_transfer.cover_checked=0;
