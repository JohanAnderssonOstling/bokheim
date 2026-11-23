-- Match account deletion: lock the owner before locking any library.
WITH owner AS MATERIALIZED (
    SELECT id FROM users WHERE id=$2 FOR KEY SHARE
)
SELECT 1
FROM libraries
WHERE id = $1 AND user_id IN (SELECT id FROM owner) AND deleted_at IS NULL
FOR UPDATE;
