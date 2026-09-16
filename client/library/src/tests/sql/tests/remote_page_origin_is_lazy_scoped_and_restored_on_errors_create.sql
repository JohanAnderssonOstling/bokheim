CREATE TEMP TABLE origin_audit(value TEXT);
      CREATE TEMP TRIGGER origin_insert AFTER INSERT ON main.sync_metadata WHEN NEW.key='change_origin' BEGIN INSERT INTO origin_audit VALUES(NEW.value); END;
      CREATE TEMP TRIGGER origin_update AFTER UPDATE ON main.sync_metadata WHEN NEW.key='change_origin' BEGIN INSERT INTO origin_audit VALUES(NEW.value); END;