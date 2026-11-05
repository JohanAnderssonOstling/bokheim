WITH input AS MATERIALIZED (
    SELECT content_hash, size_bytes, byte_checksum, ordinal
    FROM UNNEST($1::BYTEA[], $2::BIGINT[], $4::BYTEA[]) WITH ORDINALITY
         AS value(content_hash, size_bytes, byte_checksum, ordinal)
)
SELECT input.content_hash AS content_hash,
       input.size_bytes AS size_bytes,
       object.size_bytes AS stored_size_bytes,
       object.byte_checksum AS stored_checksum,
       input.byte_checksum AS requested_checksum,
       canonical.size_bytes AS canonical_size,
       EXISTS(SELECT 1 FROM user_canonical_charge c WHERE c.user_id=$3 AND c.byte_checksum=input.byte_checksum) AS canonical_owned,
       (charge.content_hash IS NOT NULL) AS owned,
       reservation.id AS reservation_id,
       reservation.library_id AS reservation_library_id,
       reservation.size_bytes AS reservation_size_bytes
FROM input
LEFT JOIN blob_object AS object
  ON object.content_hash=input.content_hash
LEFT JOIN canonical_blob_object canonical ON canonical.byte_checksum=input.byte_checksum
LEFT JOIN user_blob_charge AS charge
  ON charge.user_id=$3
 AND charge.content_hash=input.content_hash
LEFT JOIN book_upload_reservation AS reservation
  ON reservation.user_id=$3
 AND reservation.content_hash=input.content_hash
ORDER BY input.ordinal;
