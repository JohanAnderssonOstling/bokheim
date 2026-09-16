INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded, last_scan, deleted_at)
             VALUES (?1, (SELECT row_id FROM book WHERE content_hash=?2), ?3, 'fingerprint', 1, 1, NULL)
             ON CONFLICT(dir_id, book_row_id) DO UPDATE SET
                 file_name=excluded.file_name,
                 deleted_at=NULL