WITH RECURSIVE subtree(id) AS (
                SELECT id FROM dir WHERE id=?1 AND deleted_at IS NULL
                UNION ALL SELECT d.id FROM dir d JOIN subtree s ON d.parent_id=s.id WHERE d.id!=d.parent_id AND d.deleted_at IS NULL
            ) SELECT w.id FROM local_directory_work w JOIN subtree s ON s.id=w.dir_id WHERE w.dir_id!=?1