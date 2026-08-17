WITH expired_books AS (
    DELETE FROM book_upload_reservation
    WHERE user_id=$1 AND expires_at <= now()
    RETURNING size_bytes
), expired_thumbnails AS (
    DELETE FROM thumbnail_upload_reservation
    WHERE user_id=$1 AND expires_at <= now()
    RETURNING size_bytes
), expired AS (
    SELECT size_bytes FROM expired_books
    UNION ALL
    SELECT size_bytes FROM expired_thumbnails
)
SELECT COALESCE(SUM(size_bytes), 0)::BIGINT AS total_released FROM expired
