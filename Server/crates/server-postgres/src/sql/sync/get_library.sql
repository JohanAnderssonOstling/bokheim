SELECT id, name
FROM libraries
WHERE id = $1 AND user_id = $2;
