SELECT entity_key, entity_subkey, content_hash, changed_at, version_rank, replica_id, replica_seq
FROM sync_state
WHERE library_id=$1 AND kind='book_lifecycle' AND entity_key=ANY($2);
