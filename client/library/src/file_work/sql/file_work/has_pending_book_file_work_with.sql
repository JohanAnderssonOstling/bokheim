WITH RECURSIVE ancestors(id) AS (
        SELECT dir_id FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)
        UNION SELECT dir_id FROM local_file_projection WHERE content_hash=?1
        UNION SELECT d.parent_id FROM dir d JOIN ancestors a ON d.id=a.id WHERE d.id!=d.parent_id
    ) SELECT EXISTS(SELECT 1 FROM local_book_work WHERE content_hash=?1)
        OR EXISTS(SELECT 1 FROM local_file_work WHERE operation IN ('restore','trash') AND content_hash=?1)
        OR EXISTS(SELECT 1 FROM ancestors a JOIN local_directory_work w ON w.dir_id=a.id)