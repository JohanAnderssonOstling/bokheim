-- Decoded canonical registers survive removal of disposable directory rows.
CREATE TABLE local_directory_intent(id TEXT PRIMARY KEY, parent_id TEXT, name TEXT, lifecycle INTEGER);
CREATE INDEX local_directory_intent_parent ON local_directory_intent(parent_id);
CREATE INDEX local_directory_intent_lifecycle ON local_directory_intent(lifecycle);
ALTER TABLE local_directory_cache ADD COLUMN intents_valid INTEGER NOT NULL DEFAULT 0;
CREATE INDEX sync_placement_directory ON sync_state_version(state_subkey,state_key) WHERE state_kind='placement' AND body IS NOT NULL;
CREATE TABLE local_scan_changed_directory(id TEXT PRIMARY KEY, revision INTEGER NOT NULL);
CREATE INDEX local_scan_changed_directory_revision ON local_scan_changed_directory(revision);
INSERT INTO local_scan_changed_directory SELECT id,revision FROM dir CROSS JOIN local_scan_clock;
DROP TRIGGER scan_dir_insert;
DROP TRIGGER scan_dir_update;
DROP TRIGGER scan_dir_delete;
CREATE TRIGGER scan_dir_insert AFTER INSERT ON dir BEGIN
    UPDATE local_scan_clock SET revision=revision+1,tree_revision=revision+1 WHERE singleton=1;
    INSERT INTO local_scan_changed_directory SELECT NEW.id,revision FROM local_scan_clock WHERE singleton=1
        ON CONFLICT(id) DO UPDATE SET revision=excluded.revision;
END;
CREATE TRIGGER scan_dir_update AFTER UPDATE ON dir WHEN (OLD.id,OLD.parent_id,OLD.name,OLD.deleted_at) IS NOT (NEW.id,NEW.parent_id,NEW.name,NEW.deleted_at) BEGIN
    UPDATE local_scan_clock SET revision=revision+1,tree_revision=revision+1 WHERE singleton=1;
    INSERT INTO local_scan_changed_directory SELECT NEW.id,revision FROM local_scan_clock WHERE singleton=1
        ON CONFLICT(id) DO UPDATE SET revision=excluded.revision;
    INSERT INTO local_scan_changed_directory SELECT OLD.id,revision FROM local_scan_clock WHERE singleton=1
        ON CONFLICT(id) DO UPDATE SET revision=excluded.revision;
END;
CREATE TRIGGER scan_dir_delete AFTER DELETE ON dir BEGIN
    UPDATE local_scan_clock SET revision=revision+1,tree_revision=revision+1 WHERE singleton=1;
    INSERT INTO local_scan_changed_directory SELECT OLD.id,revision FROM local_scan_clock WHERE singleton=1
        ON CONFLICT(id) DO UPDATE SET revision=excluded.revision;
END;
