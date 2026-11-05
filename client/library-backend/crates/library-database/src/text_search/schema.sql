CREATE VIRTUAL TABLE IF NOT EXISTS book_text USING fts5(
    body, content='', contentless_delete=1, detail=full,
    tokenize='unicode61 remove_diacritics 2'
);
CREATE TABLE IF NOT EXISTS book_text_state(
    row_id INTEGER PRIMARY KEY REFERENCES book(row_id) ON DELETE CASCADE,
    revision TEXT NOT NULL,
    checkpoints TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS book_text_revision(singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL DEFAULT 0) STRICT;
INSERT OR IGNORE INTO book_text_revision(singleton) VALUES(1);
CREATE TRIGGER IF NOT EXISTS book_text_removed AFTER DELETE ON book_text_state
BEGIN DELETE FROM book_text WHERE rowid=OLD.row_id; END;
CREATE TRIGGER IF NOT EXISTS book_text_inserted AFTER INSERT ON book_text_state
BEGIN UPDATE book_text_revision SET revision=revision+1 WHERE singleton=1; END;
CREATE TRIGGER IF NOT EXISTS book_text_deleted AFTER DELETE ON book_text_state
BEGIN UPDATE book_text_revision SET revision=revision+1 WHERE singleton=1; END;

CREATE TRIGGER IF NOT EXISTS book_text_trashed AFTER UPDATE OF deleted_at ON book WHEN NEW.deleted_at IS NOT NULL
BEGIN DELETE FROM book_text_state WHERE row_id=NEW.row_id; END;
CREATE TRIGGER IF NOT EXISTS book_text_placement_deleted AFTER DELETE ON book_dir
BEGIN DELETE FROM book_text_state WHERE row_id=OLD.book_row_id AND NOT EXISTS(SELECT 1 FROM library_book_presence p WHERE p.book_row_id=OLD.book_row_id AND p.downloaded=1); END;
CREATE TRIGGER IF NOT EXISTS book_text_placement_updated AFTER UPDATE OF deleted_at,is_downloaded ON book_dir
BEGIN DELETE FROM book_text_state WHERE row_id=NEW.book_row_id AND NOT EXISTS(SELECT 1 FROM library_book_presence p WHERE p.book_row_id=NEW.book_row_id AND p.downloaded=1); END;
CREATE TRIGGER IF NOT EXISTS book_text_file_removed AFTER DELETE ON local_file_projection
BEGIN DELETE FROM book_text_state WHERE row_id=(SELECT row_id FROM book WHERE content_hash=OLD.content_hash) AND NOT EXISTS(SELECT 1 FROM library_book_presence p JOIN book b ON b.row_id=p.book_row_id WHERE b.content_hash=OLD.content_hash AND p.downloaded=1); END;
CREATE TRIGGER IF NOT EXISTS book_text_revision_updated AFTER UPDATE OF byte_hash ON local_file_content WHEN OLD.byte_hash IS NOT NEW.byte_hash
BEGIN DELETE FROM book_text_state WHERE row_id=(SELECT row_id FROM book WHERE content_hash=NEW.book_id); END;
CREATE TRIGGER IF NOT EXISTS book_text_revision_inserted AFTER INSERT ON local_file_content
BEGIN DELETE FROM book_text_state WHERE row_id=(SELECT row_id FROM book WHERE content_hash=NEW.book_id); END;
CREATE TRIGGER IF NOT EXISTS book_text_revision_deleted AFTER DELETE ON local_file_content
BEGIN DELETE FROM book_text_state WHERE row_id=(SELECT row_id FROM book WHERE content_hash=OLD.book_id); END;
