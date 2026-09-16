WITH RECURSIVE
candidates AS MATERIALIZED (
    SELECT w.id,w.content_hash,json_extract(j.value,'$[1]') AS check_file_work
    FROM json_each(?1) j JOIN asset_work w ON w.id=json_extract(j.value,'$[0]')
),
ancestors(work_id,id) AS MATERIALIZED (
    SELECT c.id,bd.dir_id FROM candidates c JOIN book b ON b.content_hash=c.content_hash JOIN book_dir bd ON bd.book_row_id=b.row_id
    UNION SELECT c.id,f.dir_id FROM candidates c JOIN local_file_projection f ON f.content_hash=c.content_hash
    UNION SELECT a.work_id,d.parent_id FROM ancestors a JOIN dir d ON d.id=a.id WHERE d.id!=d.parent_id
)
DELETE FROM asset_work WHERE id IN (
    SELECT c.id FROM candidates c
    WHERE NOT c.check_file_work OR (
        NOT EXISTS(SELECT 1 FROM local_book_work WHERE content_hash=c.content_hash)
        AND NOT EXISTS(SELECT 1 FROM local_file_work WHERE operation IN ('restore','trash') AND content_hash=c.content_hash)
        AND NOT EXISTS(SELECT 1 FROM ancestors a JOIN local_directory_work w ON w.dir_id=a.id WHERE a.work_id=c.id)
    )
)
