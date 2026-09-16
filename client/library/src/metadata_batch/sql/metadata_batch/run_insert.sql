INSERT INTO sync_outbox(state_kind,state_key,body,changed_at,conflict_rank)
            SELECT 'metadata',b.content_hash,
              bokheim_mutation('metadata',b.content_hash,COALESCE(b.title,''),contributors_for_book(b.row_id),b.publication_metadata),
              CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
            FROM pending_metadata_payload p JOIN book b ON b.row_id=p.book_row_id WHERE 1
            ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
              replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
              body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
            DELETE FROM pending_metadata_payload;