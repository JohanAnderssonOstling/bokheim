-- Description change triggers.
CREATE TRIGGER book_description_insert AFTER INSERT ON book_description

BEGIN
    UPDATE subject_browse_state SET revision=revision+1 WHERE id=1;
    INSERT INTO sync_outbox(state_kind,state_key,body,changed_at,conflict_rank)
    SELECT 'description',b.content_hash,bokheim_mutation('description',b.content_hash,NEW.description),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    FROM book b WHERE b.row_id=NEW.book_row_id
      AND COALESCE((SELECT value FROM sync_metadata WHERE key='change_origin'),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER book_description_update AFTER UPDATE OF description ON book_description
WHEN OLD.description IS NOT NEW.description
BEGIN
    UPDATE subject_browse_state SET revision=revision+1 WHERE id=1;
    INSERT INTO sync_outbox(state_kind,state_key,body,changed_at,conflict_rank)
    SELECT 'description',b.content_hash,bokheim_mutation('description',b.content_hash,NEW.description),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    FROM book b WHERE b.row_id=NEW.book_row_id
      AND COALESCE((SELECT value FROM sync_metadata WHERE key='change_origin'),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER book_description_delete AFTER DELETE ON book_description

BEGIN
    UPDATE subject_browse_state SET revision=revision+1 WHERE id=1;
    INSERT INTO sync_outbox(state_kind,state_key,body,changed_at,conflict_rank)
    SELECT 'description',b.content_hash,bokheim_mutation('description',b.content_hash,''),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    FROM book b WHERE b.row_id=OLD.book_row_id
      AND COALESCE((SELECT value FROM sync_metadata WHERE key='change_origin'),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;
