INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, deleted_at)
             VALUES (?1, (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'f.epub', 'old', NULL)