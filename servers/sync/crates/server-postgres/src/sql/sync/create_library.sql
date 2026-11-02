WITH changed AS (
    INSERT INTO libraries(id, user_id, name, storage_configuration, cloud_storage_enabled, kind)
    VALUES ($1, $2, $3, $4::text::jsonb, $5, $6::text::jsonb)
    ON CONFLICT (id) DO NOTHING
    RETURNING id, name, storage_configuration, kind
)
SELECT id, name, storage_configuration::text, kind::text, TRUE AS changed FROM changed
UNION ALL
SELECT id, name, storage_configuration::text, kind::text, FALSE AS changed
FROM libraries
WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
  AND kind = $6::text::jsonb AND storage_configuration = $4::text::jsonb AND NOT EXISTS(SELECT 1 FROM changed)
LIMIT 1;
