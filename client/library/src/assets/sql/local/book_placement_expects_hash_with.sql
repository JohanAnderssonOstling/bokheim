WITH RECURSIVE live_dirs(id) AS (
    SELECT id FROM dir WHERE id='00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id FROM dir d JOIN live_dirs p ON d.parent_id=p.id AND d.id!=p.id WHERE d.deleted_at IS NULL
)
SELECT b.row_id,b.content_hash FROM book b
WHERE b.row_id>?1 AND b.content_hash!='' AND EXISTS (
    SELECT 1 FROM book_dir bd JOIN live_dirs d ON d.id=bd.dir_id
    WHERE bd.book_row_id=b.row_id AND bd.deleted_at IS NULL
) ORDER BY b.row_id LIMIT 32