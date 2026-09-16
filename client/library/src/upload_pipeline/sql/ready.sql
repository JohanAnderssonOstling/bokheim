SELECT p.content_hash FROM local_upload_preparation p
LEFT JOIN local_book_upload u ON u.content_hash=p.content_hash
WHERE u.id IS NULL OR p.upload_id=u.id
