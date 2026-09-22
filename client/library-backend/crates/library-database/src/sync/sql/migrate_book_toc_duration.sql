-- The book_toc triggers now attach the audiobook duration. SQLite keeps the
-- old bodies once created, so drop and recreate exactly these two triggers;
-- every other trigger is untouched.
DROP TRIGGER IF EXISTS book_toc_insert;
DROP TRIGGER IF EXISTS book_toc_update;
CREATE TRIGGER book_toc_insert AFTER INSERT ON book_toc
BEGIN
    INSERT INTO sync_outbox(state_kind,state_key,state_subkey,body,changed_at,conflict_rank)
    SELECT 'book_toc',NEW.content_hash,'',bokheim_mutation('book_toc',NEW.content_hash,NEW.toc_json,COALESCE((SELECT duration_ms FROM audiobook_metadata WHERE content_hash=NEW.content_hash),-1)),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    WHERE COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER book_toc_update AFTER UPDATE ON book_toc
BEGIN
    INSERT INTO sync_outbox(state_kind,state_key,state_subkey,body,changed_at,conflict_rank)
    SELECT 'book_toc',NEW.content_hash,'',bokheim_mutation('book_toc',NEW.content_hash,NEW.toc_json,COALESCE((SELECT duration_ms FROM audiobook_metadata WHERE content_hash=NEW.content_hash),-1)),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    WHERE COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;
-- Stored outbox bodies use the object shape. Re-queue every stored navigation
-- once from its canonical tables, so already-queued rows match what the new
-- triggers write and audiobook navigation carries its duration.
INSERT INTO sync_outbox(state_kind,state_key,state_subkey,body,changed_at,conflict_rank)
SELECT 'book_toc',toc.content_hash,'',bokheim_mutation('book_toc',toc.content_hash,toc.toc_json,COALESCE(metadata.duration_ms,-1)),
  CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
FROM book_toc toc LEFT JOIN audiobook_metadata metadata ON metadata.content_hash=toc.content_hash
ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
  replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
  body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
