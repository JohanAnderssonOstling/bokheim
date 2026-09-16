INSERT OR REPLACE INTO local_book_work(content_hash)
SELECT value FROM json_each(?1) ORDER BY CAST(key AS INTEGER);
