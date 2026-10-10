SELECT kind, entity_key, entity_subkey, value, content_hash,
       changed_at, replica_id, replica_seq, version_rank, server_seq
FROM sync_state
WHERE library_id = $1 AND kind <> 'book_lifecycle' AND server_seq > $2 AND server_seq <= $3
ORDER BY server_seq
LIMIT $4;
