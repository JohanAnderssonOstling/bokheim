SELECT b.title IS NOT NULL,
       v.changed_at,v.conflict_rank,v.replica_id,v.replica_seq,v.mutation_id,
       COALESCE(b.purged_at IS NOT NULL,0), b.row_id IS NOT NULL
FROM json_each(?1) requested
LEFT JOIN book b ON b.content_hash=json_extract(requested.value,'$[1]')
LEFT JOIN sync_state_version v
  ON v.state_kind=json_extract(requested.value,'$[0]')
 AND v.state_key=json_extract(requested.value,'$[1]')
 AND v.state_subkey=json_extract(requested.value,'$[2]')
ORDER BY CAST(requested.key AS INTEGER);
