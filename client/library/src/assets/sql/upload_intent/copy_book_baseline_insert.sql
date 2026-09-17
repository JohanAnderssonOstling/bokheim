INSERT INTO local_book_current(dir_id,content_hash,checksum,origin)
        SELECT ?2,content_hash,checksum,origin FROM local_book_current WHERE dir_id=?1 AND content_hash=?3
        ON CONFLICT(dir_id,content_hash) DO UPDATE SET checksum=excluded.checksum,origin=excluded.origin