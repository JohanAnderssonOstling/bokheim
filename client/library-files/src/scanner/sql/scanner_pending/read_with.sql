WITH RECURSIVE pending(id) AS (
                 SELECT dir_id FROM local_directory_work
                 UNION SELECT d.id FROM dir d JOIN pending p ON d.parent_id=p.id WHERE d.id!=d.parent_id
             )
             SELECT pending.id,CASE WHEN pending.id=?1 THEN '' ELSE paths.path END,projection.relative_path
             FROM pending LEFT JOIN dir_paths paths ON paths.id=pending.id
             LEFT JOIN local_directory_projection projection ON projection.dir_id=pending.id