WITH RECURSIVE subtree(id) AS (
             SELECT id FROM dir WHERE deleted_at IS NOT NULL AND purged_at IS NULL AND intent_lifecycle=1
             UNION SELECT child.id FROM dir child JOIN subtree ON child.parent_id=subtree.id
         ) UPDATE dir SET deleted_at=?1,purged_at=?1,intent_lifecycle=2 WHERE id IN (SELECT id FROM subtree)