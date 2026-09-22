SELECT relative_path,0 AS priority FROM local_file_projection WHERE content_hash=?1
             UNION ALL
             SELECT '/' || CASE WHEN dp.path='' THEN bd.file_name ELSE dp.path || '/' || bd.file_name END,1
             FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id
             JOIN dir_paths dp ON dp.id=bd.dir_id
             WHERE b.content_hash=?1 AND b.deleted_at IS NULL AND bd.deleted_at IS NULL
             ORDER BY priority