-- Metadata coalescing triggers.
-- Version 4: description-independent metadata; connection-local batching mode; explicit upgrade definitions.
CREATE TABLE IF NOT EXISTS pending_metadata_payload(book_row_id INTEGER PRIMARY KEY REFERENCES book(row_id) ON DELETE CASCADE);
CREATE TRIGGER trig_book_metadata
AFTER UPDATE OF title, publication_metadata ON book
WHEN (OLD.title IS NOT NEW.title OR OLD.publication_metadata IS NOT NEW.publication_metadata)
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
 AND metadata_batch_flag(NULL)=0
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'metadata', NEW.content_hash,
        bokheim_mutation('metadata', NEW.content_hash, COALESCE(NEW.title, ''), contributors_for_book(NEW.row_id), NEW.publication_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER trig_book_metadata_queued
AFTER UPDATE OF title, publication_metadata ON book
WHEN (OLD.title IS NOT NEW.title OR OLD.publication_metadata IS NOT NEW.publication_metadata)
  AND COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
 AND metadata_batch_flag(NULL)=1
BEGIN
 INSERT INTO pending_metadata_payload SELECT row_id FROM book WHERE row_id=NEW.row_id ON CONFLICT(book_row_id) DO NOTHING;
END;
CREATE TRIGGER trig_book_contributor_insert
AFTER INSERT ON book_contributor
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
 AND metadata_batch_flag(NULL)=0
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    SELECT
        'metadata', book.content_hash,
        bokheim_mutation('metadata', book.content_hash, COALESCE(book.title, ''), contributors_for_book(book.row_id), book.publication_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    FROM book WHERE book.row_id = NEW.book_row_id
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER trig_book_contributor_insert_queued
AFTER INSERT ON book_contributor
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
 AND metadata_batch_flag(NULL)=1
BEGIN
 INSERT INTO pending_metadata_payload SELECT row_id FROM book WHERE row_id=NEW.book_row_id ON CONFLICT(book_row_id) DO NOTHING;
END;
CREATE TRIGGER trig_book_contributor_update
AFTER UPDATE ON book_contributor
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
 AND metadata_batch_flag(NULL)=0
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    SELECT
        'metadata', book.content_hash,
        bokheim_mutation('metadata', book.content_hash, COALESCE(book.title, ''), contributors_for_book(book.row_id), book.publication_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    FROM book WHERE book.row_id = NEW.book_row_id
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER trig_book_contributor_update_queued
AFTER UPDATE ON book_contributor
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
 AND metadata_batch_flag(NULL)=1
BEGIN
 INSERT INTO pending_metadata_payload SELECT row_id FROM book WHERE row_id=NEW.book_row_id ON CONFLICT(book_row_id) DO NOTHING;
END;
CREATE TRIGGER trig_book_contributor_delete
AFTER DELETE ON book_contributor
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
  AND EXISTS (SELECT 1 FROM book WHERE book.row_id = OLD.book_row_id)
 AND metadata_batch_flag(NULL)=0
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    SELECT
        'metadata', book.content_hash,
        bokheim_mutation('metadata', book.content_hash, COALESCE(book.title, ''), contributors_for_book(book.row_id), book.publication_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    FROM book WHERE book.row_id = OLD.book_row_id
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER trig_book_contributor_delete_queued
AFTER DELETE ON book_contributor
WHEN COALESCE((SELECT value FROM sync_metadata WHERE key = 'change_origin'), 'local') = 'local'
  AND EXISTS (SELECT 1 FROM book WHERE book.row_id = OLD.book_row_id)
 AND metadata_batch_flag(NULL)=1
BEGIN
 INSERT INTO pending_metadata_payload SELECT row_id FROM book WHERE row_id=OLD.book_row_id ON CONFLICT(book_row_id) DO NOTHING;
END;
