WITH RECURSIVE tree(id,parent_id,label,path) AS (
             SELECT id,NULL,'Library','' FROM dir WHERE id='00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
             UNION ALL
             SELECT child.id,child.parent_id,child.name,CASE WHEN tree.path='' THEN child.name ELSE tree.path||' / '||child.name END
             FROM dir child JOIN tree ON child.parent_id=tree.id
             WHERE child.id!=child.parent_id AND child.deleted_at IS NULL
         ) SELECT id,parent_id,label,path FROM tree ORDER BY portable_name_key(path),id