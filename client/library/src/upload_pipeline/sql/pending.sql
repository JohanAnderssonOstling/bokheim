SELECT EXISTS(
    SELECT 1 FROM local_book_upload u
    JOIN book b ON b.content_hash=u.content_hash
    LEFT JOIN local_upload_preparation p ON p.content_hash=u.content_hash
    WHERE b.deleted_at IS NULL AND (p.content_hash IS NULL OR p.upload_id IS NOT u.id)
)
