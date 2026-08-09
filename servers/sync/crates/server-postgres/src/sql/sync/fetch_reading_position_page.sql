SELECT kind, entity_key, entity_subkey, value, present, content_hash,
       changed_at, client_id, client_seq, version_rank, event_id, server_seq
FROM sync_reading_state
WHERE library_id = $1 AND server_seq > $2
ORDER BY server_seq
LIMIT $3;
