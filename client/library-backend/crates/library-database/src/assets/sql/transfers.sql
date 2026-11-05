-- Asset transfer pages and guarded settlements. The database owns pending work.

-- name: transfer_upload_intent?
-- param: content_hash: &BookId
SELECT upload_id, upload_size, upload_checksum FROM book_transfer WHERE content_hash=:content_hash AND upload_id IS NOT NULL;

-- name: transfer_matching_manifest?
-- param: intents: &str
SELECT MIN(requested.key) AS request_index
FROM json_each(:intents) requested
JOIN book_transfer transfer ON transfer.content_hash=unhex(json_extract(requested.value,'$[0]'))
WHERE transfer.upload_id=json_extract(requested.value,'$[1]')
  AND transfer.upload_size=json_extract(requested.value,'$[2]')
  AND transfer.upload_checksum IS unhex(json_extract(requested.value,'$[3]'))
GROUP BY transfer.content_hash ORDER BY request_index;

-- name: transfer_record_manifest_books!
-- param: intents: &str
INSERT INTO book_transfer(content_hash,server_has_book)
SELECT b.content_hash,1 FROM json_each(:intents) requested
JOIN book b ON b.content_hash=unhex(json_extract(requested.value,'$[0]'))
WHERE true
ON CONFLICT(content_hash) DO UPDATE SET server_has_book=1 WHERE book_transfer.server_has_book=0;

-- name: transfer_complete_manifest!
-- param: intents: &str
UPDATE book_transfer SET upload_id=NULL,upload_size=NULL
WHERE (content_hash,upload_id) IN (
    SELECT unhex(json_extract(value,'$[0]')),json_extract(value,'$[1]') FROM json_each(:intents)
);

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
SELECT transfer.content_hash, transfer.upload_id, transfer.upload_size, transfer.upload_checksum
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

-- name: transfer_snapshot_books?
-- param: hashes: &str
SELECT b.content_hash,b.format,b.duration_ms,CAST(b.toc_json AS BLOB),CAST(b.tracks_json AS BLOB)
FROM json_each(:hashes) requested JOIN book b ON b.content_hash=unhex(requested.value)
WHERE b.deleted_at IS NULL;

-- name: transfer_snapshot_occupied_names?
-- param: directories: &str
SELECT occupied.dir_id,occupied.file_name
FROM json_each(:directories) requested
JOIN book_dir occupied ON occupied.dir_id=requested.value AND occupied.deleted_at IS NULL
ORDER BY occupied.dir_id,occupied.book_row_id;

-- name: transfer_pending_book_uploads?
-- param: after: i64
-- param: limit: i64
SELECT transfer.upload_id, transfer.content_hash, transfer.upload_size, transfer.upload_checksum
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
-- param: after: Option<&BookId>
-- param: limit: i64
SELECT content_hash FROM uploadable_covers
WHERE NOT EXISTS(SELECT 1 FROM local_book_draft draft WHERE draft.content_hash=uploadable_covers.content_hash) AND content_hash > COALESCE(:after,X'')
ORDER BY content_hash LIMIT :limit;

-- name: transfer_complete_upload!
-- param: id: i64
-- param: content_hash: &BookId
UPDATE book_transfer SET upload_id=NULL=NULL, upload_size=NULL
WHERE content_hash=:content_hash AND upload_id=:id;

-- name: pending_thumbnail_downloads?
-- param: after: Option<&BookId>
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

-- Path eligibility needs every row for validation, but not snapshot ordering
-- or upload fingerprints. Keep these reads limited to the requested page.
-- name: thumbnail_download_projection_paths?
-- param: hashes: &str
SELECT unhex(requested.value), projection.relative_path
FROM json_each(:hashes) requested
JOIN local_file_projection projection ON projection.content_hash = unhex(requested.value);

-- name: thumbnail_download_placements?
-- param: hashes: &str
SELECT b.content_hash, bd.dir_id, bd.file_name
FROM json_each(:hashes) requested
JOIN book b ON b.content_hash = unhex(requested.value)
JOIN book_dir bd ON bd.book_row_id = b.row_id AND bd.deleted_at IS NULL
WHERE b.deleted_at IS NULL;

-- name: transfer_queue_materialization!
-- param: content_hash: &BookId
-- param: dir_id: &str
INSERT OR IGNORE INTO local_file_work(operation,content_hash,dir_id) VALUES ('materialize',:content_hash,:dir_id);

-- name: transfer_download_placement ->
-- param: content_hash: &BookId
SELECT EXISTS(SELECT 1 FROM book b JOIN book_dir bd ON bd.book_row_id=b.row_id WHERE b.content_hash=:content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL);

-- name: transfer_mark_stored_book!
-- param: content_hash: &BookId
-- param: size_bytes: i64
UPDATE book_dir SET is_downloaded=1, size_bytes=:size_bytes
WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash AND deleted_at IS NULL)
  AND deleted_at IS NULL;

-- name: transfer_observe_stored_book!
-- param: content_hash: &BookId
-- param: checksum: &ContentHash
INSERT INTO local_file_content(book_id,dir_id,byte_hash)
SELECT b.content_hash, bd.dir_id, :checksum
FROM book b JOIN book_dir bd ON bd.book_row_id=b.row_id
WHERE b.content_hash=:content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL
ON CONFLICT(book_id,dir_id) DO UPDATE SET byte_hash=excluded.byte_hash;

-- name: book_download_requests?
-- param: after_at: i64
-- param: after_hash: Option<&BookId>
-- param: limit: i64
-- Requested downloads, oldest first. Advance across failures as well as successes.
SELECT content_hash, book_download_at FROM book_transfer
WHERE book_download IS NOT NULL
  AND (book_download_at > :after_at OR (book_download_at = :after_at AND content_hash > COALESCE(:after_hash,X'')))
ORDER BY book_download_at, content_hash LIMIT :limit;

-- name: book_download_origin?
-- param: content_hash: &BookId
SELECT book_download FROM book_transfer WHERE content_hash=:content_hash AND book_download IS NOT NULL;

-- Durable download state in one row: bytes marked downloaded anywhere, and
-- a live placement with a queued request. Single-row existence checks only.
-- name: transfer_download_state ->
-- param: content_hash: &BookId
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
-- param: content_hash: &BookId
INSERT INTO book_transfer(content_hash, server_has_book, server_has_cover)
SELECT content_hash, :kind = 'book', :kind = 'thumbnail' FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET
    server_has_book = server_has_book OR excluded.server_has_book,
    server_has_cover = server_has_cover OR excluded.server_has_cover;

-- name: forget_remote_asset!
-- param: kind: &str
-- param: content_hash: &BookId
UPDATE book_transfer SET
    server_has_book = server_has_book AND :kind != 'book',
    server_has_cover = server_has_cover AND :kind != 'thumbnail'
WHERE content_hash = :content_hash;

-- name: remote_asset_hashes?
-- param: kind: &str
SELECT content_hash FROM book_transfer WHERE CASE :kind WHEN 'book' THEN server_has_book ELSE server_has_cover END;

-- name: record_cover_presence_present!
-- param: hashes: &str
INSERT INTO book_transfer(content_hash, server_has_cover)
SELECT b.content_hash, 1
FROM json_each(:hashes) requested
JOIN book b ON b.content_hash = unhex(requested.value)
WHERE true
ON CONFLICT(content_hash) DO UPDATE SET server_has_cover = 1
WHERE book_transfer.server_has_cover = 0;

-- name: record_cover_presence_missing!
-- param: hashes: &str
UPDATE book_transfer SET server_has_cover = 0
WHERE server_has_cover = 1
  AND content_hash IN (SELECT unhex(value) FROM json_each(:hashes));

-- name: complete_cover_downloads_ready!
-- param: hashes: &str
INSERT INTO book_transfer(content_hash, cover_state)
SELECT b.content_hash, 'ready'
FROM json_each(:hashes) requested
JOIN book b ON b.content_hash = unhex(requested.value)
WHERE true
ON CONFLICT(content_hash) DO UPDATE SET cover_state = 'ready'
WHERE book_transfer.cover_state IS NOT 'ready';


-- name: asset_clear_downloaded_placements !
-- param: content_hash: &SqlParam<'_>
UPDATE book_dir SET is_downloaded=0,local_hash='',size_bytes=NULL WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: asset_clear_book_paths !
-- param: content_hash: &SqlParam<'_>
DELETE FROM local_file_projection WHERE content_hash=:content_hash;
