-- Reconciles locally scanned files with the stored placement inventory.

-- name: mark_vanished_book_dirs_unavailable!
-- param: scan_id: i64
-- param: grace: i64
-- Mark locally-materialized placements unavailable after the grace window.
-- This is device-local availability only and emits no deletion mutation.
UPDATE book_dir
SET is_downloaded = 0
WHERE deleted_at IS NULL
  AND is_downloaded = 1
  AND last_scan > 0
  AND (:scan_id - last_scan) >= :grace;

-- name: count_pending_vanish_candidates?
-- param: scan_id: i64
-- param: grace: i64
-- Placements missing now but not yet past the grace window. While any exist a
-- re-scan is scheduled so the miss count advances even if nothing else triggers a
-- scan (deletions are event-driven, so one removal would otherwise stall at one miss).
SELECT COUNT(*) FROM book_dir
WHERE deleted_at IS NULL
  AND is_downloaded = 1
  AND last_scan > 0
  AND last_scan < :scan_id
  AND (:scan_id - last_scan) < :grace;

-- name: get_known_directory_scan_state?
-- param: dir_id: &str
SELECT directory.deleted_at IS NULL, paths.path
FROM dir directory
LEFT JOIN dir_paths paths ON paths.id = directory.id
WHERE directory.id = :dir_id;

-- name: is_book_file_tombstoned?
-- param: dir_id: &str
-- param: file_name: &str
SELECT
    EXISTS(
        SELECT 1 FROM book_dir removed
        WHERE removed.dir_id = :dir_id AND removed.file_name = :file_name
          AND removed.deleted_at IS NOT NULL
    )
    AND NOT EXISTS(
        SELECT 1 FROM book_dir live
        WHERE live.dir_id = :dir_id AND live.file_name = :file_name
          AND live.deleted_at IS NULL
    );
-- name: mark_known_book_dir_seen!
-- param: scan_id: i64
-- param: dir_id: &str
-- param: file_name: &str
-- param: local_hash: &str
UPDATE book_dir
SET last_scan = :scan_id
WHERE dir_id = :dir_id
  AND file_name = :file_name
  AND local_hash = :local_hash
  AND deleted_at IS NULL;

-- name: book_has_live_placement?
-- param: dir_id: &str
-- param: book_id: &str
SELECT EXISTS(
    SELECT 1
    FROM book_dir
    WHERE dir_id = :dir_id
      AND book_id = :book_id
      AND deleted_at IS NULL
);
