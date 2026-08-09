INSERT INTO libraries(id, user_id)
VALUES ($1, $2)
ON CONFLICT (id) DO UPDATE SET user_id = libraries.user_id
RETURNING user_id;
