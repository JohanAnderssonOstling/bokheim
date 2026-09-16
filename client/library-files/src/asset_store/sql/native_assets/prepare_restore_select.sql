SELECT max(current.checksum)
        FROM local_publication_current current JOIN book b ON b.content_hash=current.content_hash
        JOIN book_dir bd ON bd.book_row_id=b.row_id AND bd.dir_id=current.dir_id AND bd.deleted_at IS NULL
        JOIN dir_paths dp ON dp.id=bd.dir_id
        WHERE current.content_hash=?1 AND '/' || CASE WHEN dp.path='' THEN bd.file_name ELSE dp.path||'/'||bd.file_name END=?2