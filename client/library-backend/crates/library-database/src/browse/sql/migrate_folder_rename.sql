DROP TRIGGER IF EXISTS folder_dirty_directory_update;
CREATE TRIGGER IF NOT EXISTS folder_dirty_directory_update AFTER UPDATE OF parent_id,name,deleted_at,purged_at ON dir
WHEN OLD.parent_id IS NOT NEW.parent_id OR (OLD.name LIKE '.%') IS NOT (NEW.name LIKE '.%') OR OLD.deleted_at IS NOT NEW.deleted_at OR OLD.purged_at IS NOT NEW.purged_at
BEGIN UPDATE browse_projection_state SET dirty=1 WHERE projection='folder_ownership'; END;
