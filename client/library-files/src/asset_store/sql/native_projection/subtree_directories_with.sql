WITH RECURSIVE subtree(id,depth) AS (
        SELECT id,0 FROM dir WHERE id=?1 AND deleted_at IS NULL
        UNION ALL SELECT d.id,s.depth+1 FROM dir d JOIN subtree s ON d.parent_id=s.id WHERE d.id!=d.parent_id AND d.deleted_at IS NULL
    ) SELECT id FROM subtree ORDER BY depth,id