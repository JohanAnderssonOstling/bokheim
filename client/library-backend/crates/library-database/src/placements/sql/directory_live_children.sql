SELECT id FROM dir WHERE parent_id IN (SELECT value FROM json_each(?1)) AND id!=parent_id AND deleted_at IS NULL;
