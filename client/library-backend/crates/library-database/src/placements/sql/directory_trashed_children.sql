SELECT id FROM dir WHERE COALESCE(intent_parent_id,parent_id) IN (SELECT value FROM json_each(?1)) AND id!=COALESCE(intent_parent_id,parent_id) AND deleted_at IS NOT NULL;
