-- Version 62: empty new directories cannot change existing membership.
DROP TRIGGER IF EXISTS folder_dirty_directory_insert;
