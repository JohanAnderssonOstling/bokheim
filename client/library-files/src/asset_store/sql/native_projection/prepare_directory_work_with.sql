WITH RECURSIVE ancestry(token,dir_id,id,parent_id,depth) AS (
        SELECT w.id,w.dir_id,d.id,d.parent_id,0 FROM local_directory_work w LEFT JOIN dir d ON d.id=w.dir_id
        UNION ALL SELECT a.token,a.dir_id,d.id,d.parent_id,a.depth+1 FROM ancestry a JOIN dir d ON d.id=a.parent_id WHERE a.id!=a.parent_id
    ) SELECT token,dir_id FROM ancestry GROUP BY token,dir_id ORDER BY MAX(depth),token