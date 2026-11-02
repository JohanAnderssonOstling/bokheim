SELECT id, name, storage_configuration::text, kind::text
FROM libraries
WHERE user_id = $1 AND deleted_at IS NULL
ORDER BY lower(name), id;
