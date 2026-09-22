-- Membership already includes each visible book and all its ancestor routes.
DROP TRIGGER IF EXISTS trig_subject_book_ownership_insert;
DROP TRIGGER IF EXISTS trig_subject_book_ownership_delete;
DROP TABLE IF EXISTS subject_book_ownership;
DELETE FROM browse_projection_state WHERE projection='subject_ownership';
