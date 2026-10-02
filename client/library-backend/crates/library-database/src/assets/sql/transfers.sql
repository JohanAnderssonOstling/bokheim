-- Asset transfer planning and settlement. Per-book state is one
-- `book_transfer` row. A rejection recorded after `rejected_after` still
-- holds its upload back.

-- name: transfer_upload_intent?
-- param: content_hash: &str
SELECT upload_id, upload_checksum, upload_size FROM book_transfer WHERE content_hash=:content_hash AND upload_id IS NOT NULL;

-- Hash-level transfer snapshot reads. Each query returns only rows for the
-- requested hashes; one-to-many values are grouped by the Rust assembler.
-- name: transfer_snapshot_versions?
-- param: hashes: &str
SELECT b.content_hash, bd.dir_id, bd.file_name, bd.local_hash
FROM json_each(:hashes) requested
JOIN book b ON b.content_hash = requested.value
JOIN book_dir bd ON bd.book_row_id = b.row_id AND bd.deleted_at IS NULL
WHERE b.deleted_at IS NULL
ORDER BY b.content_hash, bd.dir_id, bd.file_name;

-- name: transfer_snapshot_intents?
-- param: hashes: &str
SELECT transfer.content_hash, transfer.upload_id, transfer.upload_checksum, transfer.upload_size
FROM json_each(:hashes) requested
JOIN book_transfer transfer ON transfer.content_hash = requested.value
WHERE transfer.upload_id IS NOT NULL;

-- name: transfer_snapshot_projection_paths?
-- param: hashes: &str
-- Where each requested book was last materialized on disk.
SELECT requested.value, projection.relative_path
FROM json_each(:hashes) requested
JOIN local_file_projection projection ON projection.content_hash = requested.value
ORDER BY requested.value;

-- name: transfer_snapshot_thumbnail_allowed?
-- param: hashes: &str
-- param: rejected_after: i64
-- A cover is set once and any device holding it may upload it, but only
-- for a book the server already holds whose lifecycle has been published.
-- The pending upload is not required: it is removed once the book's bytes
-- are uploaded.
SELECT requested.value, EXISTS(
    SELECT 1 FROM uploadable_covers cover WHERE cover.content_hash=requested.value
      AND NOT COALESCE(cover.cover_rejected_at > :rejected_after, 0))
FROM json_each(:hashes) requested;

-- name: transfer_pending_book_uploads?
-- param: after: i64
-- param: limit: i64
-- param: rejected_after: i64
SELECT transfer.upload_id, transfer.content_hash, transfer.upload_checksum, transfer.upload_size
FROM book_transfer transfer
JOIN book b ON b.content_hash = transfer.content_hash AND b.deleted_at IS NULL
WHERE transfer.upload_id > :after AND transfer.upload_prepared = transfer.upload_id
  AND EXISTS(SELECT 1 FROM book_dir bd WHERE bd.book_row_id = b.row_id AND bd.deleted_at IS NULL)
  AND NOT COALESCE(transfer.upload_rejected_at > :rejected_after, 0)
ORDER BY transfer.upload_id LIMIT :limit;

-- Covers are derived work, not an intent queue: select them directly from
-- authoritative state. The upload snapshot repeats this predicate only as a
-- last-moment safety check before bytes are sent.
-- name: transfer_pending_cover_uploads?
-- param: limit: i64
-- param: rejected_after: i64
SELECT content_hash FROM uploadable_covers
WHERE NOT COALESCE(cover_rejected_at > :rejected_after, 0)
ORDER BY content_hash LIMIT :limit;

-- name: transfer_complete_upload!
-- param: id: i64
-- param: content_hash: &str
-- param: checksum: &str
UPDATE book_transfer SET upload_id=NULL, upload_checksum=NULL, upload_size=NULL
WHERE content_hash=:content_hash AND upload_id=:id AND upload_checksum=:checksum;

-- name: transfer_record_book_rejection!
-- param: content_hash: &str
-- param: reason: &str
INSERT INTO book_transfer(content_hash, upload_rejected_reason, upload_rejected_at)
SELECT content_hash, :reason, unixepoch() FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET upload_rejected_reason = excluded.upload_rejected_reason, upload_rejected_at = excluded.upload_rejected_at;

-- name: transfer_record_thumbnail_rejection!
-- param: content_hash: &str
INSERT INTO book_transfer(content_hash, cover_rejected_at)
SELECT content_hash, unixepoch() FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET cover_rejected_at = excluded.cover_rejected_at;

-- name: pending_thumbnail_downloads?
-- param: after: &str
-- param: scope: Option<&str>
-- param: limit: i64
-- Books whose cover this device should fetch: no local thumbnail and none
-- being generated, and either the server has one or the book's bytes are not
-- here to generate one from. `scope` limits the page to a JSON list of hashes.
SELECT b.content_hash FROM book b
LEFT JOIN book_transfer transfer ON transfer.content_hash = b.content_hash
WHERE b.content_hash > :after AND b.content_hash != '' AND b.deleted_at IS NULL
  AND (:scope IS NULL OR b.content_hash IN (SELECT value FROM json_each(:scope)))
  AND COALESCE(transfer.cover_state NOT IN ('ready', 'pending'), 1)
  AND (COALESCE(transfer.server_has_cover, 0)
       OR NOT EXISTS(SELECT 1 FROM library_book_presence presence WHERE presence.book_row_id=b.row_id AND presence.downloaded=1))
ORDER BY b.content_hash LIMIT :limit;

-- name: transfer_queue_materialization!
-- param: content_hash: &str
-- param: relative_path: &str
INSERT OR IGNORE INTO local_file_work(operation,content_hash,relative_path) VALUES ('materialize',:content_hash,:relative_path);

-- name: transfer_download_placement?
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book b JOIN book_dir bd ON bd.book_row_id=b.row_id WHERE b.content_hash=:content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL);

-- name: book_download_requests?
-- Requested downloads, oldest first, and who asked for each.
SELECT content_hash, book_download FROM book_transfer
WHERE book_download IS NOT NULL
ORDER BY book_download_at, content_hash;

-- Durable download state in one row: bytes marked downloaded anywhere, and
-- a live placement with a queued request. Single-row existence checks only.
-- name: transfer_download_state?
-- param: content_hash: &str
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

-- name: transfer_uploads_waiting_for_storage ->
-- param: rejected_after: i64
SELECT EXISTS (
    SELECT 1 FROM book_transfer
    WHERE upload_id IS NOT NULL
      AND upload_rejected_reason = 'server rejected upload admission: QuotaExceeded'
      AND upload_rejected_at > :rejected_after
);

-- name: transfer_first_file_names?
-- param: hashes: &str
SELECT requested.value, (SELECT bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash=requested.value ORDER BY bd.file_name LIMIT 1)
FROM json_each(:hashes) requested;

-- name: transfer_book_upload_rejection_reason?
-- param: content_hash: &str
-- param: rejected_after: i64
SELECT upload_rejected_reason FROM book_transfer
WHERE content_hash = :content_hash AND upload_id IS NOT NULL AND upload_rejected_at > :rejected_after;

-- name: transfer_has_pending_asset_work?
-- param: live_dirs: &str
-- param: rejected_after: i64
-- `live_dirs`: every live folder, from Rust.
WITH live_dirs(id) AS (SELECT value FROM json_each(:live_dirs)), live_books AS (
    SELECT b.content_hash,
        EXISTS(SELECT 1 FROM library_book_presence presence
            WHERE presence.book_row_id=b.row_id AND presence.downloaded=1) AS local
    FROM book b WHERE b.content_hash!='' AND EXISTS(
        SELECT 1 FROM book_dir bd JOIN live_dirs d ON d.id=bd.dir_id
        WHERE bd.book_row_id=b.row_id AND bd.deleted_at IS NULL)
)
SELECT EXISTS(SELECT 1 FROM book_transfer WHERE book_download='user_initiated')
OR EXISTS(
    SELECT 1 FROM live_books b
    LEFT JOIN book_transfer t ON t.content_hash=b.content_hash
    WHERE (b.local AND t.upload_id IS NOT NULL AND NOT COALESCE(t.upload_rejected_at>:rejected_after, 0))
        OR EXISTS(SELECT 1 FROM uploadable_covers cover WHERE cover.content_hash=b.content_hash
                  AND NOT COALESCE(cover.cover_rejected_at>:rejected_after, 0))
        OR (COALESCE(t.cover_state NOT IN ('ready','pending'), 1) AND (NOT b.local OR COALESCE(t.server_has_cover, 0)))
);

-- name: record_remote_asset!
-- param: kind: &str
-- param: content_hash: &str
INSERT INTO book_transfer(content_hash, server_has_book, server_has_cover)
SELECT content_hash, :kind = 'book', :kind = 'thumbnail' FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET
    server_has_book = server_has_book OR excluded.server_has_book,
    server_has_cover = server_has_cover OR excluded.server_has_cover;

-- name: forget_remote_asset!
-- param: kind: &str
-- param: content_hash: &str
UPDATE book_transfer SET
    server_has_book = server_has_book AND :kind != 'book',
    server_has_cover = server_has_cover AND :kind != 'thumbnail'
WHERE content_hash = :content_hash;

-- name: remote_asset_hashes?
-- param: kind: &str
SELECT content_hash FROM book_transfer WHERE CASE :kind WHEN 'book' THEN server_has_book ELSE server_has_cover END;
