-- A blocked file replay must not report progress merely because a scan
-- refreshed its observations. Count completed work, independently of scans.
ALTER TABLE local_scan_clock ADD COLUMN replay_revision INTEGER NOT NULL DEFAULT 0;
CREATE TRIGGER replay_progress_local_file_work AFTER DELETE ON local_file_work BEGIN
    UPDATE local_scan_clock SET replay_revision=replay_revision+1 WHERE singleton=1;
END;
CREATE TRIGGER replay_progress_local_book_work AFTER DELETE ON local_book_work BEGIN
    UPDATE local_scan_clock SET replay_revision=replay_revision+1 WHERE singleton=1;
END;
CREATE TRIGGER replay_progress_local_directory_work AFTER DELETE ON local_directory_work BEGIN
    UPDATE local_scan_clock SET replay_revision=replay_revision+1 WHERE singleton=1;
END;
CREATE TRIGGER replay_progress_local_purge_work AFTER DELETE ON local_purge_work BEGIN
    UPDATE local_scan_clock SET replay_revision=replay_revision+1 WHERE singleton=1;
END;
CREATE TRIGGER replay_progress_local_directory_projection AFTER DELETE ON local_directory_projection BEGIN
    UPDATE local_scan_clock SET replay_revision=replay_revision+1 WHERE singleton=1;
END;
