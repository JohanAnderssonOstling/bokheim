SELECT b.content_hash, u.id
FROM book b LEFT JOIN local_book_upload u ON u.content_hash=b.content_hash
WHERE b.deleted_at IS NULL
ORDER BY b.content_hash
