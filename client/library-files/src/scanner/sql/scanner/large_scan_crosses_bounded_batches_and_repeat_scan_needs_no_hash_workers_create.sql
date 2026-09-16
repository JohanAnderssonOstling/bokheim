CREATE TABLE dir_write_audit(operation TEXT NOT NULL);
             CREATE TRIGGER audit_dir_insert AFTER INSERT ON dir BEGIN
                 INSERT INTO dir_write_audit(operation) VALUES ('insert');
             END;
             CREATE TRIGGER audit_dir_update AFTER UPDATE ON dir BEGIN
                 INSERT INTO dir_write_audit(operation) VALUES ('update');
             END;