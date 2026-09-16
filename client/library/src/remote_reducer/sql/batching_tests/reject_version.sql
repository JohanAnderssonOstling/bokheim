CREATE TRIGGER reject_batch_version BEFORE INSERT ON sync_state_version
WHEN NEW.state_kind = 'book_lifecycle'
AND (SELECT COUNT(*) FROM sync_state_version WHERE state_kind = 'book_lifecycle') >= 100
BEGIN SELECT RAISE(ABORT, 'injected version failure'); END;
