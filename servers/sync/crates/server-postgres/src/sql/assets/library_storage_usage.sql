-- Per-library breakdown of the bytes an account is charged for.
--
-- The quota charges each canonical checksum once, while different logical
-- book IDs can refer to those same bytes from several
-- libraries. Summing per library would therefore double count shared books and
-- overrun the quota, so each charged blob is attributed to exactly one library:
-- the lowest library id that still holds an unpurged book_lifecycle reference to
-- it. The choice is arbitrary but stable, so a book does not move between
-- libraries in the breakdown from one reading to the next.
--
-- Blobs that are still charged but referenced by no library — a book removed
-- everywhere whose bytes have not yet been released — are attributed to no
-- library and appear as the difference between this total and used_bytes.
WITH referenced AS (
    SELECT DISTINCT state.library_id, state.content_hash AS content_hash
    FROM sync_state AS state
    JOIN libraries AS library
      ON library.id = state.library_id
     AND library.user_id = $1
     AND library.deleted_at IS NULL
     AND library.cloud_storage_enabled
    WHERE state.kind = 'book_lifecycle'
      AND state.version_rank <> 2
      AND state.content_hash IS NOT NULL
    UNION
    SELECT claim.library_id, claim.content_hash FROM library_blob_claim claim
    JOIN libraries library ON library.id=claim.library_id AND library.deleted_at IS NULL AND library.cloud_storage_enabled
    WHERE claim.user_id=$1
),
owning AS (
    SELECT object.byte_checksum,MIN(referenced.library_id) AS library_id
    FROM referenced
    JOIN user_blob_charge ownership ON ownership.user_id=$1 AND ownership.content_hash=referenced.content_hash
    JOIN blob_object object ON object.content_hash=referenced.content_hash
    JOIN user_canonical_charge charge ON charge.user_id=$1 AND charge.byte_checksum=object.byte_checksum
    GROUP BY object.byte_checksum
), book_usage AS (
    SELECT owning.library_id,COALESCE(SUM(object.size_bytes),0)::BIGINT AS used_bytes
    FROM owning JOIN canonical_blob_object object USING(byte_checksum)
    GROUP BY owning.library_id
)
SELECT library_id,used_bytes FROM book_usage ORDER BY library_id;
