SELECT present, content_hash, changed_at, version_rank, client_id, client_seq, event_id
FROM sync_state
WHERE library_id = $1
  AND kind = 'book_lifecycle'
  AND entity_key = $2
  AND entity_subkey = ''
