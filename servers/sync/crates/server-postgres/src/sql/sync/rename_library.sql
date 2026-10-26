WITH changed AS (
    UPDATE libraries SET name = $3
    WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
      AND name IS DISTINCT FROM $3
    RETURNING id, name, storage_configuration
)
SELECT id, name, storage_configuration::text, TRUE AS changed FROM changed
UNION ALL
SELECT id, name, storage_configuration::text, FALSE AS changed FROM libraries
WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
  AND NOT EXISTS(SELECT 1 FROM changed)
LIMIT 1;
