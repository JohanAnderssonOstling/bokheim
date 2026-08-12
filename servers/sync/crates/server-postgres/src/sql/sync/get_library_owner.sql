SELECT user_id
FROM libraries
WHERE id = $1 AND deleted_at IS NULL;
