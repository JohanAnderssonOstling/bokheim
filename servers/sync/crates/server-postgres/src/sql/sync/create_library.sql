INSERT INTO libraries(id, user_id, name, storage_configuration, cloud_storage_enabled)
VALUES ($1, $2, $3, $4::text::jsonb, $5)
ON CONFLICT (id) DO NOTHING
RETURNING id, name, storage_configuration::text, TRUE AS changed;
