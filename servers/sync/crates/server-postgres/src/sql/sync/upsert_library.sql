WITH changed AS (
    INSERT INTO libraries(id, user_id, name)
    VALUES ($1, $2, $3)
    ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name
    WHERE libraries.user_id = EXCLUDED.user_id
      AND libraries.name IS DISTINCT FROM EXCLUDED.name
    RETURNING id, name
)
SELECT id, name, TRUE AS changed FROM changed
UNION ALL
SELECT id, name, FALSE AS changed
FROM libraries
WHERE id = $1 AND user_id = $2 AND NOT EXISTS(SELECT 1 FROM changed)
LIMIT 1;
