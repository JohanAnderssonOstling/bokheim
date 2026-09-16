SELECT p.relative_path FROM local_file_projection p
             JOIN book b ON b.content_hash=p.content_hash WHERE b.deleted_at IS NULL
             UNION
             SELECT '/' || CASE WHEN dp.path='' THEN bd.file_name ELSE dp.path || '/' || bd.file_name END
             FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id
             JOIN dir_paths dp ON dp.id=bd.dir_id
             WHERE b.deleted_at IS NULL AND bd.deleted_at IS NULL