WITH RECURSIVE affected_dirs(id) AS (
    SELECT dir_id FROM local_directory_work
    UNION SELECT d.id FROM dir d JOIN affected_dirs a ON d.parent_id=a.id WHERE d.id!=d.parent_id
)
SELECT b.content_hash,u.id
FROM json_each(?1) members JOIN book b ON b.content_hash=members.value
LEFT JOIN local_book_upload u ON u.content_hash=b.content_hash
WHERE b.deleted_at IS NULL
AND NOT EXISTS(SELECT 1 FROM local_book_work w WHERE w.content_hash=b.content_hash)
AND NOT EXISTS(SELECT 1 FROM local_file_work w WHERE w.content_hash=b.content_hash AND w.operation IN ('restore','trash'))
AND NOT EXISTS(SELECT 1 FROM book_dir bd JOIN affected_dirs d ON d.id=bd.dir_id WHERE bd.book_row_id=b.row_id)
AND NOT EXISTS(SELECT 1 FROM local_file_projection fp JOIN affected_dirs d ON d.id=fp.dir_id WHERE fp.content_hash=b.content_hash)
ORDER BY b.content_hash
