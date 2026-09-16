WITH pending(content_hash) AS (SELECT content_hash FROM local_book_work UNION SELECT content_hash FROM local_file_work)
             SELECT p.relative_path FROM local_file_projection p JOIN pending USING(content_hash)
             UNION ALL SELECT relative_path FROM local_file_work
             UNION ALL
             SELECT '/' || CASE WHEN dp.path='' THEN bd.file_name ELSE dp.path || '/' || bd.file_name END
             FROM pending JOIN book b USING(content_hash) JOIN book_dir bd ON bd.book_row_id=b.row_id
             JOIN dir_paths dp ON dp.id=bd.dir_id
             WHERE b.deleted_at IS NULL AND bd.deleted_at IS NULL