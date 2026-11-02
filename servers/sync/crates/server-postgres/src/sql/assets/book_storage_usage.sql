-- Choose one stable logical book to represent each charged canonical file.
WITH charged AS (
    SELECT DISTINCT ON (charge.byte_checksum) ownership.content_hash,object.size_bytes AS used_bytes
    FROM user_canonical_charge charge
    JOIN canonical_blob_object object USING(byte_checksum)
    JOIN blob_object book ON book.byte_checksum=charge.byte_checksum
    JOIN user_blob_charge ownership ON ownership.user_id=charge.user_id AND ownership.content_hash=book.content_hash
    WHERE charge.user_id=$1
    ORDER BY charge.byte_checksum,ownership.content_hash
)
SELECT content_hash,used_bytes FROM charged ORDER BY used_bytes DESC,content_hash LIMIT $2;
