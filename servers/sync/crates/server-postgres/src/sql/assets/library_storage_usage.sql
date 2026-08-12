-- Per-library breakdown of the bytes an account is charged for.
--
-- The quota charges each unique blob once (user_blob_charge is keyed by user
-- and content hash), but the same book may be referenced from several
-- libraries. Summing per library would therefore double count shared books and
-- overrun the quota, so each charged blob is attributed to exactly one library:
-- the lowest library id that still holds a present book_lifecycle reference to
-- it. The choice is arbitrary but stable, so a book does not move between
-- libraries in the breakdown from one reading to the next.
--
-- Blobs that are still charged but referenced by no library — a book removed
-- everywhere whose bytes have not yet been released — are attributed to no
-- library and appear as the difference between this total and used_bytes.
WITH referenced AS (
    SELECT DISTINCT state.library_id, state.content_hash
    FROM sync_state AS state
    JOIN libraries AS library
      ON library.id = state.library_id
     AND library.user_id = $1
     AND library.deleted_at IS NULL
    WHERE state.kind = 'book_lifecycle'
      AND state.present
      AND state.content_hash IS NOT NULL
),
owning AS (
    SELECT referenced.content_hash, MIN(referenced.library_id) AS library_id
    FROM referenced
    JOIN user_blob_charge AS charge
      ON charge.user_id = $1
     AND charge.content_hash = referenced.content_hash
    GROUP BY referenced.content_hash
)
SELECT owning.library_id AS library_id,
       COALESCE(SUM(object.size_bytes), 0)::BIGINT AS used_bytes
FROM owning
JOIN blob_object AS object
  ON object.content_hash = owning.content_hash
GROUP BY owning.library_id
ORDER BY owning.library_id;
