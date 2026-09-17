-- Resumable subject-enrichment checkpoints and bounded ISBN-response cache.

-- name: subject_pipeline_candidates?
-- param: after: &str
-- param: limit: i64
-- param: inspection_revision: i64
-- param: scope: Option<&str>
SELECT b.content_hash, COALESCE(b.title, ''), b.format, b.book_metadata,
       CASE WHEN s.metadata = b.book_metadata AND s.title = COALESCE(b.title, '')
                 AND NOT(s.retry_at > 0 AND COALESCE(json_extract(s.state, '$.inspection_revision'), 0) <> :inspection_revision)
                 AND NOT(s.complete = 1 AND s.retry_at > 0 AND s.retry_at <= CAST(strftime('%s', 'now') AS INTEGER))
            THEN s.state ELSE NULL END
FROM book b LEFT JOIN local_subject_pipeline s USING(content_hash)
WHERE (:scope IS NULL OR b.content_hash IN (SELECT value FROM json_each(:scope)))
  AND b.deleted_at IS NULL AND b.hidden_at IS NULL AND b.content_hash > :after
  AND NOT EXISTS(SELECT 1 FROM pending_audible_enrichment a WHERE a.content_hash = b.content_hash)
  AND (s.content_hash IS NULL OR s.metadata <> b.book_metadata OR s.title <> COALESCE(b.title, '')
       OR (s.retry_at > 0 AND COALESCE(json_extract(s.state, '$.inspection_revision'), 0) <> :inspection_revision)
       OR ((s.complete = 0 OR s.retry_at > 0) AND s.retry_at <= CAST(strftime('%s', 'now') AS INTEGER)))
ORDER BY b.content_hash LIMIT :limit;

-- name: subject_pipeline_book ->
-- param: content_hash: &str
SELECT book_metadata FROM book WHERE content_hash = :content_hash;

-- name: save_subject_pipeline_checkpoint!
-- param: content_hash: &str
-- param: state: &str
-- param: complete: bool
-- param: retry_seconds: i64
-- param: expected: &[u8]
-- param: title: &str
-- param: empty_metadata: bool
INSERT INTO local_subject_pipeline(content_hash, state, metadata, title, complete, retry_at)
SELECT content_hash, :state, book_metadata, COALESCE(title, ''), :complete,
       CASE WHEN :complete = 1 AND :retry_seconds = 0 THEN 0 ELSE CAST(strftime('%s', 'now') AS INTEGER) + :retry_seconds END
FROM book WHERE content_hash = :content_hash AND deleted_at IS NULL
  AND (book_metadata = :expected OR (:empty_metadata AND length(book_metadata) = 0))
  AND COALESCE(title, '') = :title
ON CONFLICT(content_hash) DO UPDATE SET state = excluded.state, metadata = excluded.metadata,
    title = excluded.title, complete = excluded.complete, retry_at = excluded.retry_at;

-- name: subject_pipeline_step_succeeded ->
-- param: content_hash: &str
-- param: provider: &str
SELECT status <> 'failed' FROM external_metadata_attempt
WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash = :content_hash)
  AND provider_id = :provider;

-- name: cached_isbn_response ->
-- param: isbn: &str
SELECT response FROM local_isbn_response
WHERE isbn = :isbn AND expires_at > CAST(strftime('%s', 'now') AS INTEGER);

-- name: cache_isbn_response!
-- param: isbn: &str
-- param: response: &str
INSERT INTO local_isbn_response VALUES (:isbn, :response, CAST(strftime('%s', 'now') AS INTEGER) + 2592000)
ON CONFLICT(isbn) DO UPDATE SET response = excluded.response, expires_at = excluded.expires_at;

-- name: trim_isbn_response_cache!
DELETE FROM local_isbn_response
WHERE isbn IN (SELECT isbn FROM local_isbn_response ORDER BY expires_at DESC, isbn LIMIT -1 OFFSET 4096);
