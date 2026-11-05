INSERT INTO blob_object(content_hash,size_bytes,byte_checksum) VALUES($1,$2,$3)
ON CONFLICT(content_hash) DO UPDATE SET size_bytes=blob_object.size_bytes
WHERE blob_object.size_bytes=excluded.size_bytes
  AND blob_object.byte_checksum=excluded.byte_checksum
RETURNING content_hash
