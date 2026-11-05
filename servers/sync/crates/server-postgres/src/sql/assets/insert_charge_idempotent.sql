WITH ownership AS (
    INSERT INTO user_blob_charge(user_id,content_hash) VALUES($1,$2)
    ON CONFLICT(user_id,content_hash) DO NOTHING
)
INSERT INTO user_canonical_charge(user_id,byte_checksum)
SELECT $1,byte_checksum FROM blob_object WHERE content_hash=$2
ON CONFLICT(user_id,byte_checksum) DO NOTHING;
