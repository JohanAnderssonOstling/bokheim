-- Asset transfer pages and guarded settlements. The database owns pending work.

-- name: transfer_upload_intent?
-- param: content_hash: &ContentHash
SELECT upload_id, upload_size FROM book_transfer WHERE content_hash=:content_hash AND upload_id IS NOT NULL;

-- Hash-level transfer snapshot reads. Each query returns only rows for the
-- requested hashes; one-to-many values are grouped by the Rust assembler.
-- name: transfer_snapshot_versions?
-- param: hashes: &str
SELECT b.content_hash, bd.dir_id, bd.file_name, bd.local_hash
FROM json_each(:hashes) requested
JOIN book b ON b.content_hash = unhex(requested.value)
JOIN book_dir bd ON bd.book_row_id = b.row_id AND bd.deleted_at IS NULL
WHERE b.deleted_at IS NULL
ORDER BY b.content_hash, bd.dir_id, bd.file_name;

-- name: transfer_snapshot_intents?
-- param: hashes: &str
SELECT transfer.content_hash, transfer.upload_id, transfer.upload_size
FROM json_each(:hashes) requested
JOIN book_transfer transfer ON transfer.content_hash = unhex(requested.value)
WHERE transfer.upload_id IS NOT NULL
  AND NOT EXISTS(SELECT 1 FROM local_book_draft draft WHERE draft.content_hash=transfer.content_hash);

-- name: transfer_snapshot_projection_paths?
-- param: hashes: &str
-- Where each requested book was last materialized on disk.
SELECT unhex(requested.value), projection.relative_path
FROM json_each(:hashes) requested
JOIN local_file_projection projection ON projection.content_hash = unhex(requested.value)
ORDER BY requested.value;

-- name: transfer_pending_book_uploads?
-- param: after: i64
-- param: limit: i64
SELECT transfer.upload_id, transfer.content_hash, transfer.upload_size
FROM book_transfer transfer
JOIN book b ON b.content_hash = transfer.content_hash AND b.deleted_at IS NULL
WHERE transfer.upload_id > :after AND transfer.upload_prepared = transfer.upload_id
  AND NOT EXISTS(SELECT 1 FROM local_book_draft draft WHERE draft.content_hash=transfer.content_hash)
  AND EXISTS(SELECT 1 FROM book_dir bd WHERE bd.book_row_id = b.row_id AND bd.deleted_at IS NULL)
ORDER BY transfer.upload_id LIMIT :limit;

-- Covers are derived work, not an intent queue: select them directly from
-- authoritative state. The upload snapshot repeats this predicate only as a
-- last-moment safety check before bytes are sent.
-- name: transfer_pending_cover_uploads?
-- param: after: Option<&ContentHash>
-- param: limit: i64
SELECT content_hash FROM uploadable_covers
WHERE NOT EXISTS(SELECT 1 FROM local_book_draft draft WHERE draft.content_hash=uploadable_covers.content_hash) AND content_hash > COALESCE(:after,X'')
ORDER BY content_hash LIMIT :limit;

-- name: transfer_complete_upload!
-- param: id: i64
-- param: content_hash: &ContentHash
UPDATE book_transfer SET upload_id=NULL=NULL, upload_size=NULL
WHERE content_hash=:content_hash AND upload_id=:id;

-- name: pending_thumbnail_downloads?
-- param: after: Option<&ContentHash>
-- param: limit: i64
-- Books whose cover this device should fetch: no local thumbnail and none
-- being generated, and either the server has one or the book's bytes are not
-- here to generate one from.
SELECT b.content_hash FROM book b
LEFT JOIN book_transfer transfer ON transfer.content_hash = b.content_hash
WHERE b.content_hash > COALESCE(:after,X'') AND b.content_hash != '' AND b.deleted_at IS NULL
  AND COALESCE(transfer.cover_state NOT IN ('ready', 'pending'), 1)
  AND (COALESCE(transfer.server_has_cover, 0)
       OR NOT EXISTS(SELECT 1 FROM library_book_presence presence WHERE presence.book_row_id=b.row_id AND presence.downloaded=1))
ORDER BY b.content_hash LIMIT :limit;

-- name: transfer_queue_materialization!
-- param: content_hash: &ContentHash
-- param: relative_path: &str
INSERT OR IGNORE INTO local_file_work(operation,content_hash,relative_path) VALUES ('materialize',:content_hash,:relative_path);

-- name: transfer_download_placement?
-- param: content_hash: &ContentHash
SELECT EXISTS(SELECT 1 FROM book b JOIN book_dir bd ON bd.book_row_id=b.row_id WHERE b.content_hash=:content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL);

-- name: book_download_requests?
-- param: after_at: i64
-- param: after_hash: Option<&ContentHash>
-- param: limit: i64
-- Requested downloads, oldest first. Advance across failures as well as successes.
SELECT content_hash, book_download_at FROM book_transfer
WHERE book_download IS NOT NULL
  AND (book_download_at > :after_at OR (book_download_at = :after_at AND content_hash > COALESCE(:after_hash,X'')))
ORDER BY book_download_at, content_hash LIMIT :limit;

-- Durable download state in one row: bytes marked downloaded anywhere, and
-- a live placement with a queued request. Single-row existence checks only.
-- name: transfer_download_state?
-- param: content_hash: &ContentHash
SELECT
    EXISTS (
        SELECT 1 FROM library_book_presence presence
        WHERE presence.book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash)
          AND presence.downloaded=1
    ),
    EXISTS (
        SELECT 1
        FROM book_transfer transfer
        JOIN book ON book.content_hash = transfer.content_hash
        JOIN book_dir placement ON placement.book_row_id = book.row_id AND placement.deleted_at IS NULL
        WHERE transfer.content_hash = :content_hash AND transfer.book_download IS NOT NULL
    );

-- name: record_remote_asset!
-- param: kind: &str
-- param: content_hash: &ContentHash
INSERT INTO book_transfer(content_hash, server_has_book, server_has_cover)
SELECT content_hash, :kind = 'book', :kind = 'thumbnail' FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET
    server_has_book = server_has_book OR excluded.server_has_book,
    server_has_cover = server_has_cover OR excluded.server_has_cover;

-- name: forget_remote_asset!
-- param: kind: &str
-- param: content_hash: &ContentHash
UPDATE book_transfer SET
    server_has_book = server_has_book AND :kind != 'book',
    server_has_cover = server_has_cover AND :kind != 'thumbnail'
WHERE content_hash = :content_hash;

-- name: remote_asset_hashes?
-- param: kind: &str
SELECT content_hash FROM book_transfer WHERE CASE :kind WHEN 'book' THEN server_has_book ELSE server_has_cover END;


-- name: asset_clear_downloaded_placements !
-- param: content_hash: &SqlParam<'_>
UPDATE book_dir SET is_downloaded=0,local_hash='',size_bytes=NULL WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: asset_clear_book_paths !
-- param: content_hash: &SqlParam<'_>
DELETE FROM local_file_projection WHERE content_hash=:content_hash;
