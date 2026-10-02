SELECT COALESCE(revision.current_revision, 0)
FROM libraries AS library
LEFT JOIN sync_library_revision AS revision ON revision.library_id = library.id
WHERE library.id = $1 AND library.user_id = $2 AND library.deleted_at IS NULL;
