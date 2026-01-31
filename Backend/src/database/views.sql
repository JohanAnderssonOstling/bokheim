-- name: create_views!
CREATE VIEW IF NOT EXISTS dir_paths AS
WITH RECURSIVE dir_path(id, path) AS (
    -- root produces empty path
    SELECT id, ''
    FROM dir
    WHERE parent_id = id
      AND deleted_at IS NULL

    UNION ALL

    -- children append normally
    SELECT
        d.id,
        CASE
            WHEN dp.path = ''
                THEN d.name
            ELSE dp.path || '/' || d.name
        END
    FROM dir d
    JOIN dir_path dp ON d.parent_id = dp.id
    WHERE d.deleted_at IS NULL
      AND d.id <> d.parent_id
)
SELECT id, path FROM dir_path;


