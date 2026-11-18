SELECT EXISTS (
    SELECT 1 FROM sync_state
    WHERE library_id=$1 AND kind='book_lifecycle'
      AND version_rank<>2 AND content_hash=$3
)
FROM libraries
WHERE id=$1 AND user_id=$2 AND deleted_at IS NULL;
