SELECT id FROM dir WHERE id IN (SELECT value FROM json_each(?1)) AND deleted_at IS NOT NULL;
