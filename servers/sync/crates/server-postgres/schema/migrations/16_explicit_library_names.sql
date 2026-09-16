-- Existing libraries keep their names; new libraries require explicit names.
ALTER TABLE libraries ALTER COLUMN name DROP DEFAULT;
