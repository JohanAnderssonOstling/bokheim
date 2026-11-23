INSERT INTO library_blob_claim(library_id, user_id, content_hash)
SELECT $1, $2, content_hash FROM UNNEST($3::bytea[]) AS hashes(content_hash)
ON CONFLICT(library_id, content_hash) DO NOTHING;
