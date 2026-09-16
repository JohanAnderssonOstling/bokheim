CREATE TRIGGER reject_metadata_version BEFORE INSERT ON sync_state_version
WHEN NEW.state_kind='metadata' AND (SELECT COUNT(*) FROM sync_state_version WHERE state_kind='metadata') >= 100
BEGIN SELECT RAISE(ABORT, 'injected metadata version failure'); END;
