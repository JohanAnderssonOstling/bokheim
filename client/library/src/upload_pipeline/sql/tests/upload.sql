INSERT INTO local_book_upload(content_hash,checksum,size_bytes) VALUES (?1,?1,10)
ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id
