-- Canonicalize an upload batch under its caller's writer transaction.

-- name: content_validate_batch?
-- param: contents: &str
SELECT current.canonical_hash,current.size_bytes,
       unhex(json_extract(requested.value,'$[1]')),json_extract(requested.value,'$[2]')
FROM json_each(:contents) requested
LEFT JOIN book_content current ON current.book_id=unhex(json_extract(requested.value,'$[0]'));

-- name: content_freeze_batch!
-- param: contents: &str
INSERT INTO book_content(book_id,canonical_hash,size_bytes)
SELECT unhex(json_extract(value,'$[0]')),unhex(json_extract(value,'$[1]')),json_extract(value,'$[2]')
FROM json_each(:contents) WHERE true
ON CONFLICT(book_id) DO UPDATE SET size_bytes=excluded.size_bytes
WHERE book_content.size_bytes IS NULL AND excluded.size_bytes IS NOT NULL;

-- name: content_update_uploads_batch!
-- param: contents: &str
UPDATE book_transfer AS transfer
SET upload_checksum=unhex(json_extract(requested.value,'$[1]')),
    upload_size=COALESCE(json_extract(requested.value,'$[2]'),transfer.upload_size)
FROM json_each(:contents) requested
WHERE transfer.content_hash=unhex(json_extract(requested.value,'$[0]'))
  AND transfer.upload_id IS NOT NULL
  AND (transfer.upload_checksum IS NOT unhex(json_extract(requested.value,'$[1]'))
       OR (json_extract(requested.value,'$[2]') IS NOT NULL
           AND transfer.upload_size IS NOT json_extract(requested.value,'$[2]')));

-- name: content_forget_origins_batch!
-- param: contents: &str
DELETE FROM local_content_origin
WHERE book_id IN (SELECT unhex(json_extract(value,'$[0]')) FROM json_each(:contents));

-- name: content_record_aliases_batch!
-- param: contents: &str
INSERT OR IGNORE INTO local_content_alias(byte_hash,book_id)
SELECT unhex(json_extract(value,'$[1]')),unhex(json_extract(value,'$[0]')) FROM json_each(:contents);
