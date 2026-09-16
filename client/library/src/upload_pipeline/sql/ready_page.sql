SELECT p.content_hash FROM json_each(?1) requested
JOIN local_upload_preparation p ON p.content_hash=requested.value
LEFT JOIN local_book_upload u ON u.content_hash=p.content_hash
WHERE u.id IS NULL OR p.upload_id=u.id
