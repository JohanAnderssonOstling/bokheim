-- Read snapshots and stale-write guards for prepared enrichment batches.

-- name: enrichment_snapshot ->
-- param: content_hash: &str
SELECT row_id, book_metadata, COALESCE(title, ''),
       COALESCE((SELECT description FROM book_description WHERE book_row_id = book.row_id), ''),
       contributors_for_book(row_id)
FROM book WHERE content_hash = :content_hash AND deleted_at IS NULL AND hidden_at IS NULL;

-- name: enrichment_snapshot_is_current ->
-- param: content_hash: &str
-- param: row_id: i64
-- param: book: &[u8]
-- param: title: &str
-- param: description: &str
-- param: contributors: &[u8]
SELECT EXISTS(
    SELECT 1 FROM book
    WHERE content_hash = :content_hash AND row_id = :row_id AND book_metadata = :book
      AND COALESCE(title, '') = :title
      AND COALESCE((SELECT description FROM book_description WHERE book_row_id = book.row_id), '') = :description
      AND contributors_for_book(row_id) = :contributors
      AND deleted_at IS NULL AND hidden_at IS NULL
);

-- name: non_authoritative_contributors?
-- param: content_hash: &str
SELECT ai.stable_id, bc.name, bc.sort_name, bc.role
FROM book_contributor bc JOIN author_identity ai ON ai.id = bc.author_identity_id
WHERE bc.book_row_id = (SELECT row_id FROM book WHERE content_hash = :content_hash)
  AND bc.role <> X'617574'
ORDER BY bc.position;
