SELECT json_array('book',content_hash,title,added_at,format,deleted_at,purged_at,hex(publication_metadata)) AS value FROM book
UNION ALL
SELECT json_array('version',state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id)
FROM sync_state_version WHERE state_key IN (SELECT content_hash FROM book)
UNION ALL
SELECT json_array('file_work',id,content_hash) FROM local_book_work
UNION ALL
SELECT json_array('asset_work',content_hash) FROM asset_work
UNION ALL
SELECT json_array('contributor',b.content_hash,c.position,c.name,c.sort_name,hex(c.role))
FROM book_contributor c JOIN book b ON b.row_id=c.book_row_id
ORDER BY value;
