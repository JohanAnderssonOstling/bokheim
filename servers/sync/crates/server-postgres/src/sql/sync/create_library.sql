WITH changed AS (
    INSERT INTO libraries(id, user_id, name, storage_configuration, cloud_storage_enabled)
    VALUES ($1, $2, $3, $4::text::jsonb, $5)
    ON CONFLICT (id) DO NOTHING
    RETURNING id, name, storage_configuration
)
SELECT id, name, storage_configuration::text, TRUE AS changed FROM changed
UNION ALL
SELECT id, name, storage_configuration::text, FALSE AS changed
FROM libraries
WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
  AND storage_configuration = $4::text::jsonb AND NOT EXISTS(SELECT 1 FROM changed)
LIMIT 1;
