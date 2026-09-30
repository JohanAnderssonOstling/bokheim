SELECT id FROM dir WHERE parent_id=?1 AND name=?2 AND deleted_at IS NULL AND id!=parent_id;
