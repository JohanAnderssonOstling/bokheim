WITH expired_books AS (
    DELETE FROM book_upload_reservation
    WHERE user_id=$1 AND expires_at <= now()
    RETURNING size_bytes
), released AS (
    SELECT COALESCE(SUM(size_bytes), 0)::bigint AS size_bytes FROM expired_books
)
UPDATE user_storage_account AS account
SET reserved_bytes=account.reserved_bytes-released.size_bytes
FROM released
WHERE account.user_id=$1 AND released.size_bytes>0;
