SELECT (
    EXISTS(SELECT 1 FROM user_canonical_charge WHERE user_id=$1 AND byte_checksum=$3)
    OR EXISTS(SELECT 1 FROM user_blob_charge WHERE user_id=$1 AND content_hash=$2)
) AS owned,
EXISTS(SELECT 1 FROM book_upload_reservation WHERE user_id=$1 AND content_hash=$2) AS reserved;
