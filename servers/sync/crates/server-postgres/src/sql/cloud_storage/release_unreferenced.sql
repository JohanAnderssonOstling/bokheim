WITH released AS (
 DELETE FROM user_blob_charge charge USING blob_object object
 WHERE charge.user_id=$1 AND object.content_hash=charge.content_hash
 AND NOT EXISTS(SELECT 1 FROM library_blob_claim claim WHERE claim.user_id=$1 AND claim.content_hash=charge.content_hash)
 AND NOT EXISTS(SELECT 1 FROM user_blob_reference ref
                WHERE ref.user_id=$1 AND ref.content_hash=charge.content_hash)
 RETURNING charge.content_hash,object.byte_checksum
), released_canonical AS (
 DELETE FROM user_canonical_charge charge USING canonical_blob_object object
 WHERE charge.user_id=$1 AND object.byte_checksum=charge.byte_checksum
 AND charge.byte_checksum IN (SELECT byte_checksum FROM released)
 AND NOT EXISTS(
   SELECT 1 FROM user_blob_charge ownership JOIN blob_object book ON book.content_hash=ownership.content_hash
   WHERE ownership.user_id=$1 AND book.byte_checksum=charge.byte_checksum
   AND ownership.content_hash NOT IN (SELECT content_hash FROM released)
 )
 RETURNING object.size_bytes
)
UPDATE user_storage_account SET used_bytes=used_bytes-COALESCE((SELECT SUM(size_bytes) FROM released_canonical),0) WHERE user_id=$1;
