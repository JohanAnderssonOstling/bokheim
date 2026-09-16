SELECT '/' || CASE WHEN dp.path='' THEN bd.file_name ELSE dp.path || '/' || bd.file_name END,p.relative_path
             FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id
             JOIN dir_paths dp ON dp.id=bd.dir_id
             LEFT JOIN local_file_projection p ON p.content_hash=b.content_hash AND p.dir_id=bd.dir_id
             WHERE b.content_hash=?1 AND b.deleted_at IS NULL AND bd.deleted_at IS NULL