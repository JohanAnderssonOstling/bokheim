SELECT bd.file_name,bd.local_hash,'/' || CASE WHEN dp.path='' THEN bd.file_name ELSE dp.path || '/' || bd.file_name END
             FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id JOIN dir_paths dp ON dp.id=bd.dir_id
             WHERE b.content_hash=?1 AND bd.dir_id=?2 AND bd.deleted_at IS NULL AND b.deleted_at IS NULL