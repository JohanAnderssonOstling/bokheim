SELECT COALESCE(revision.current_revision, 0) AS current_revision, history.generation
FROM libraries AS library
LEFT JOIN sync_library_revision AS revision ON revision.library_id = library.id
CROSS JOIN sync_history AS history
WHERE library.id = $1 AND library.deleted_at IS NULL
FOR SHARE OF history;
