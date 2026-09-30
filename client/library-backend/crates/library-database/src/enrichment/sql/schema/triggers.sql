-- name: schema_enrichment_triggers &
CREATE TRIGGER trig_book_metadata
AFTER UPDATE OF title, subtitle, book_metadata ON book
WHEN (OLD.title IS NOT NEW.title OR OLD.subtitle IS NOT NEW.subtitle OR OLD.book_metadata IS NOT NEW.book_metadata)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
 AND metadata_batch_flag(NULL)=0
BEGIN
    SELECT bokheim_accept_local(bokheim_mutation('metadata', NEW.content_hash, COALESCE(NEW.title, ''), NEW.subtitle, contributors_for_book(NEW.row_id), NEW.book_metadata), CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER));
END;

CREATE TRIGGER trig_book_metadata_queued
AFTER UPDATE OF title, subtitle, book_metadata ON book
WHEN (OLD.title IS NOT NEW.title OR OLD.subtitle IS NOT NEW.subtitle OR OLD.book_metadata IS NOT NEW.book_metadata)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
 AND metadata_batch_flag(NULL)=1
BEGIN
 INSERT INTO pending_metadata_payload SELECT row_id FROM book WHERE row_id=NEW.row_id ON CONFLICT(book_row_id) DO NOTHING;
END;

CREATE TRIGGER trig_book_contributor_insert
AFTER INSERT ON book_contributor
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
 AND metadata_batch_flag(NULL)=0
BEGIN
    SELECT bokheim_accept_local(bokheim_mutation('metadata', book.content_hash, COALESCE(book.title, ''), book.subtitle, contributors_for_book(book.row_id), book.book_metadata), CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)) FROM book WHERE book.row_id = NEW.book_row_id;
END;

CREATE TRIGGER trig_book_contributor_insert_queued
AFTER INSERT ON book_contributor
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
 AND metadata_batch_flag(NULL)=1
BEGIN
 INSERT INTO pending_metadata_payload SELECT row_id FROM book WHERE row_id=NEW.book_row_id ON CONFLICT(book_row_id) DO NOTHING;
END;

CREATE TRIGGER trig_book_contributor_update
AFTER UPDATE ON book_contributor
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
 AND metadata_batch_flag(NULL)=0
BEGIN
    SELECT bokheim_accept_local(bokheim_mutation('metadata', book.content_hash, COALESCE(book.title, ''), book.subtitle, contributors_for_book(book.row_id), book.book_metadata), CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)) FROM book WHERE book.row_id = NEW.book_row_id;
END;

CREATE TRIGGER trig_book_contributor_update_queued
AFTER UPDATE ON book_contributor
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
 AND metadata_batch_flag(NULL)=1
BEGIN
 INSERT INTO pending_metadata_payload SELECT row_id FROM book WHERE row_id=NEW.book_row_id ON CONFLICT(book_row_id) DO NOTHING;
END;

CREATE TRIGGER trig_book_contributor_delete
AFTER DELETE ON book_contributor
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
  AND EXISTS (SELECT 1 FROM book WHERE book.row_id = OLD.book_row_id)
 AND metadata_batch_flag(NULL)=0
BEGIN
    SELECT bokheim_accept_local(bokheim_mutation('metadata', book.content_hash, COALESCE(book.title, ''), book.subtitle, contributors_for_book(book.row_id), book.book_metadata), CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)) FROM book WHERE book.row_id = OLD.book_row_id;
END;

CREATE TRIGGER trig_book_contributor_delete_queued
AFTER DELETE ON book_contributor
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
  AND EXISTS (SELECT 1 FROM book WHERE book.row_id = OLD.book_row_id)
 AND metadata_batch_flag(NULL)=1
BEGIN
 INSERT INTO pending_metadata_payload SELECT row_id FROM book WHERE row_id=OLD.book_row_id ON CONFLICT(book_row_id) DO NOTHING;
END;
