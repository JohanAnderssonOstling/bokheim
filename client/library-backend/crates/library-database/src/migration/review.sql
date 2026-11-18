-- Schema 57: bounded transfer allocation and indexed child lookups.
CREATE TABLE upload_clock(singleton INTEGER PRIMARY KEY CHECK(singleton=1), sequence INTEGER NOT NULL CHECK(sequence>=0)) STRICT;
INSERT INTO upload_clock SELECT 1,COALESCE(MAX(MAX(COALESCE(upload_id,0),COALESCE(upload_prepared,0))),0) FROM book_transfer;
-- Track all writers, including bulk policy refresh and restored transfer state.
CREATE TRIGGER upload_clock_insert AFTER INSERT ON book_transfer BEGIN
    UPDATE upload_clock SET sequence=MAX(sequence,COALESCE(NEW.upload_id,0),COALESCE(NEW.upload_prepared,0)) WHERE singleton=1 AND sequence<MAX(COALESCE(NEW.upload_id,0),COALESCE(NEW.upload_prepared,0));
END;
CREATE TRIGGER upload_clock_update AFTER UPDATE OF upload_id,upload_prepared ON book_transfer BEGIN
    UPDATE upload_clock SET sequence=MAX(sequence,COALESCE(NEW.upload_id,0),COALESCE(NEW.upload_prepared,0)) WHERE singleton=1 AND sequence<MAX(COALESCE(NEW.upload_id,0),COALESCE(NEW.upload_prepared,0));
END;
CREATE INDEX idx_sync_book_record_content ON sync_book_record(content_hash);
CREATE INDEX idx_local_content_alias_book ON local_content_alias(book_id);
CREATE INDEX idx_local_file_work_content ON local_file_work(content_hash);
CREATE INDEX idx_local_file_content_directory ON local_file_content(dir_id);
CREATE INDEX idx_author_name_key_identity ON author_name_key(author_identity_id);

-- Projection and cleanup paths use exactly one leading slash.
UPDATE local_file_projection SET relative_path='/'||ltrim(relative_path,'/') WHERE relative_path!='/'||ltrim(relative_path,'/');
UPDATE local_directory_projection SET relative_path='/'||ltrim(relative_path,'/') WHERE relative_path!='/'||ltrim(relative_path,'/');
UPDATE OR REPLACE local_file_work SET relative_path='/'||ltrim(relative_path,'/') WHERE relative_path IS NOT NULL AND relative_path!='/'||ltrim(relative_path,'/');

CREATE INDEX idx_book_content_checksum ON book_content(canonical_hash);
CREATE TABLE local_legacy_book_id(legacy_id BLOB PRIMARY KEY, book_id BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE local_purged_directory(id TEXT PRIMARY KEY) STRICT, WITHOUT ROWID;
CREATE TABLE local_directory_cache(singleton INTEGER PRIMARY KEY CHECK(singleton=1), valid INTEGER NOT NULL) STRICT;
INSERT INTO local_directory_cache VALUES(1,0);

-- Scanner guards track all inventory mutations, including ones that do not
-- enqueue native work. Keep the last revision per book for bounded refreshes.
CREATE TABLE local_scan_clock(singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL, tree_revision INTEGER NOT NULL) STRICT;
INSERT INTO local_scan_clock VALUES(1,1,1);
CREATE TABLE local_scan_changed_book(content_hash BLOB PRIMARY KEY, revision INTEGER NOT NULL) STRICT, WITHOUT ROWID;
CREATE INDEX idx_local_scan_changed_revision ON local_scan_changed_book(revision);
CREATE TRIGGER scan_book_insert AFTER INSERT ON book BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
    INSERT INTO local_scan_changed_book SELECT NEW.content_hash,revision FROM local_scan_clock WHERE singleton=1
        ON CONFLICT(content_hash) DO UPDATE SET revision=excluded.revision;
END;
CREATE TRIGGER scan_book_update AFTER UPDATE ON book WHEN OLD.content_hash IS NOT NEW.content_hash OR OLD.deleted_at IS NOT NEW.deleted_at BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
    INSERT INTO local_scan_changed_book SELECT OLD.content_hash,revision FROM local_scan_clock WHERE singleton=1
        ON CONFLICT(content_hash) DO UPDATE SET revision=excluded.revision;
    INSERT INTO local_scan_changed_book SELECT NEW.content_hash,revision FROM local_scan_clock WHERE singleton=1
        ON CONFLICT(content_hash) DO UPDATE SET revision=excluded.revision;
END;
CREATE TRIGGER scan_book_delete BEFORE DELETE ON book BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
    INSERT INTO local_scan_changed_book SELECT OLD.content_hash,revision FROM local_scan_clock WHERE singleton=1
        ON CONFLICT(content_hash) DO UPDATE SET revision=excluded.revision;
END;
CREATE TRIGGER scan_book_dir_insert AFTER INSERT ON book_dir BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
    INSERT INTO local_scan_changed_book SELECT b.content_hash,c.revision FROM book b CROSS JOIN local_scan_clock c WHERE b.row_id=NEW.book_row_id AND c.singleton=1
        ON CONFLICT(content_hash) DO UPDATE SET revision=excluded.revision;
END;
CREATE TRIGGER scan_book_dir_update AFTER UPDATE ON book_dir BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
    INSERT INTO local_scan_changed_book SELECT b.content_hash,c.revision FROM book b CROSS JOIN local_scan_clock c WHERE b.row_id=OLD.book_row_id AND c.singleton=1
        ON CONFLICT(content_hash) DO UPDATE SET revision=excluded.revision;
    INSERT INTO local_scan_changed_book SELECT b.content_hash,c.revision FROM book b CROSS JOIN local_scan_clock c WHERE b.row_id=NEW.book_row_id AND c.singleton=1
        ON CONFLICT(content_hash) DO UPDATE SET revision=excluded.revision;
END;
CREATE TRIGGER scan_book_dir_delete AFTER DELETE ON book_dir BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
    INSERT INTO local_scan_changed_book SELECT b.content_hash,c.revision FROM book b CROSS JOIN local_scan_clock c WHERE b.row_id=OLD.book_row_id AND c.singleton=1
        ON CONFLICT(content_hash) DO UPDATE SET revision=excluded.revision;
END;
CREATE TRIGGER scan_local_file_projection_insert AFTER INSERT ON local_file_projection BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
    INSERT INTO local_scan_changed_book SELECT NEW.content_hash,revision FROM local_scan_clock WHERE singleton=1
        ON CONFLICT(content_hash) DO UPDATE SET revision=excluded.revision;
END;
CREATE TRIGGER scan_local_file_projection_update AFTER UPDATE ON local_file_projection BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
    INSERT INTO local_scan_changed_book SELECT OLD.content_hash,revision FROM local_scan_clock WHERE singleton=1
        ON CONFLICT(content_hash) DO UPDATE SET revision=excluded.revision;
    INSERT INTO local_scan_changed_book SELECT NEW.content_hash,revision FROM local_scan_clock WHERE singleton=1
        ON CONFLICT(content_hash) DO UPDATE SET revision=excluded.revision;
END;
CREATE TRIGGER scan_local_file_projection_delete AFTER DELETE ON local_file_projection BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
    INSERT INTO local_scan_changed_book SELECT OLD.content_hash,revision FROM local_scan_clock WHERE singleton=1
        ON CONFLICT(content_hash) DO UPDATE SET revision=excluded.revision;
END;
CREATE TRIGGER scan_dir_insert AFTER INSERT ON dir BEGIN
    UPDATE local_scan_clock SET revision=revision+1,tree_revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER scan_dir_update AFTER UPDATE ON dir WHEN (OLD.id,OLD.parent_id,OLD.name,OLD.deleted_at) IS NOT (NEW.id,NEW.parent_id,NEW.name,NEW.deleted_at) BEGIN
    UPDATE local_scan_clock SET revision=revision+1,tree_revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER scan_dir_delete AFTER DELETE ON dir BEGIN
    UPDATE local_scan_clock SET revision=revision+1,tree_revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER scan_local_directory_work_insert AFTER INSERT ON local_directory_work BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER scan_local_directory_work_update AFTER UPDATE ON local_directory_work BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER scan_local_directory_work_delete AFTER DELETE ON local_directory_work BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER scan_local_book_work_insert AFTER INSERT ON local_book_work BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER scan_local_book_work_update AFTER UPDATE ON local_book_work BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER scan_local_book_work_delete AFTER DELETE ON local_book_work BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER scan_local_file_work_insert AFTER INSERT ON local_file_work BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER scan_local_file_work_update AFTER UPDATE ON local_file_work BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER scan_local_file_work_delete AFTER DELETE ON local_file_work BEGIN
    UPDATE local_scan_clock SET revision=revision+1 WHERE singleton=1;
END;
