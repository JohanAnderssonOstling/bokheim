WITH released AS (
    DELETE FROM book_upload_reservation
    WHERE id=$1 AND user_id=$2
    RETURNING size_bytes
)
UPDATE user_storage_account AS account
SET reserved_bytes=account.reserved_bytes-released.size_bytes
FROM released
WHERE account.user_id=$2;
