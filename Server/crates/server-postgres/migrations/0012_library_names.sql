ALTER TABLE libraries ADD COLUMN IF NOT EXISTS name text;

UPDATE libraries SET name = 'Library' WHERE name IS NULL OR btrim(name) = '';

ALTER TABLE libraries ALTER COLUMN name SET DEFAULT 'Library';
ALTER TABLE libraries ALTER COLUMN name SET NOT NULL;
