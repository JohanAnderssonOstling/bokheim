SELECT object.byte_checksum
FROM libraries AS library
LEFT JOIN user_blob_charge AS charge
  ON charge.user_id=library.user_id AND charge.content_hash=$3
LEFT JOIN blob_object AS object ON object.content_hash=charge.content_hash
WHERE library.id=$1 AND library.user_id=$2
  AND library.deleted_at IS NULL AND library.cloud_storage_enabled;
