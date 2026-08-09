SELECT id, name
FROM libraries
WHERE user_id = $1
ORDER BY lower(name), id;
