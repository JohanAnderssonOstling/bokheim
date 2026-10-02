SELECT id FROM dir WHERE COALESCE(canonical_parent_id,parent_id) IN (SELECT value FROM json_each(?1)) AND id!=COALESCE(canonical_parent_id,parent_id) AND deleted_at IS NOT NULL;
