UPDATE book_dir SET last_scan=?1
WHERE deleted_at IS NULL AND (dir_id,file_name,local_hash) IN (
    SELECT json_extract(value,'$[0]'),json_extract(value,'$[1]'),json_extract(value,'$[2]')
    FROM json_each(?2)
)
