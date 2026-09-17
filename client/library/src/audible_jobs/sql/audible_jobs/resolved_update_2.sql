INSERT INTO book_toc(content_hash,entry_count,toc_json) VALUES(?1,?2,?3)
ON CONFLICT(content_hash) DO UPDATE SET entry_count=excluded.entry_count,toc_json=excluded.toc_json
