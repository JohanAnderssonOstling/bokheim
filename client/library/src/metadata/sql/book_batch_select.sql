SELECT b.book_metadata
FROM json_each(?1) requested
LEFT JOIN book b ON b.content_hash=requested.value
ORDER BY CAST(requested.key AS INTEGER);
