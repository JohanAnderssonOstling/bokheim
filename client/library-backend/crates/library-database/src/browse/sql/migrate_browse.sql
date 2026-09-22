-- Version 61 replaces only rebuildable browse data and its maintenance.
DROP TRIGGER IF EXISTS trig_folder_book_ownership_insert;
DROP TRIGGER IF EXISTS trig_folder_book_ownership_update_remove;
DROP TRIGGER IF EXISTS trig_folder_book_ownership_update_add;
DROP TRIGGER IF EXISTS trig_folder_book_ownership_delete;
DROP TRIGGER IF EXISTS trig_folder_book_ownership_directory_update;
DROP TRIGGER IF EXISTS trig_folder_book_ownership_directory_delete;
DROP TABLE folder_book_ownership;
UPDATE browse_projection_state SET dirty=1 WHERE projection='folder_ownership';
DROP VIEW book_cards;
