DELETE FROM sync_outbox; CREATE TEMP TABLE payload_audit(n INTEGER);
            CREATE TEMP TRIGGER payload_insert AFTER INSERT ON main.sync_outbox WHEN NEW.state_kind='metadata' BEGIN INSERT INTO payload_audit VALUES(1); END;
            CREATE TEMP TRIGGER payload_update AFTER UPDATE OF body ON main.sync_outbox WHEN NEW.state_kind='metadata' BEGIN INSERT INTO payload_audit VALUES(1); END;