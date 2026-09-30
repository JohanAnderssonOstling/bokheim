-- name: schema_native_triggers &
CREATE TRIGGER IF NOT EXISTS pdf_reader_metadata_insert AFTER INSERT ON pdf_reader_metadata
BEGIN
    SELECT bokheim_accept_local(bokheim_mutation('pdf_reader_metadata',NEW.content_hash,NEW.metadata), CAST((julianday('now')-2440587.5)*86400000 AS INTEGER)) WHERE COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local';
END;

CREATE TRIGGER IF NOT EXISTS pdf_reader_metadata_update AFTER UPDATE ON pdf_reader_metadata
BEGIN
    SELECT bokheim_accept_local(bokheim_mutation('pdf_reader_metadata',NEW.content_hash,NEW.metadata), CAST((julianday('now')-2440587.5)*86400000 AS INTEGER)) WHERE COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local';
END;

-- Deleting a book cancels its pending cover; restoring it asks for one again.
-- Writers of a pending cover skip deleted books themselves.
CREATE TRIGGER IF NOT EXISTS thumbnail_cancel_deleted_book AFTER UPDATE OF deleted_at ON book
WHEN NEW.deleted_at IS NOT NULL
BEGIN UPDATE book_transfer SET cover_state=NULL,cover_retry_after=0 WHERE content_hash=NEW.content_hash AND cover_state='pending'; END;

CREATE TRIGGER IF NOT EXISTS thumbnail_restore_book AFTER UPDATE OF deleted_at ON book
WHEN OLD.deleted_at IS NOT NULL AND NEW.deleted_at IS NULL
BEGIN
    INSERT INTO book_transfer(content_hash,cover_state) VALUES(NEW.content_hash,'pending')
    ON CONFLICT(content_hash) DO UPDATE SET cover_state='pending' WHERE cover_state IS NULL;
END;
