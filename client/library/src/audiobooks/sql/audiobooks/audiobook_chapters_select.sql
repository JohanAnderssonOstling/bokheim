SELECT toc.toc_json, metadata.duration_ms FROM book_toc toc
JOIN audiobook_metadata metadata ON metadata.content_hash = toc.content_hash
WHERE toc.content_hash=?1
