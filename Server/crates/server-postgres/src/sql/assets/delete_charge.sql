DELETE FROM user_blob_charge
WHERE user_id = $1 AND content_hash = $2;
