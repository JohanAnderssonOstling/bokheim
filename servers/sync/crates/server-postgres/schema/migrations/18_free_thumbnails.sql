-- Run with the old server stopped, through upgrade_existing's transaction.
-- Thumbnail files remain on disk; only their obsolete billing records go away.
-- The migration marker makes the refund happen exactly once.
LOCK TABLE user_storage_account, user_thumbnail_charge, thumbnail_upload_reservation IN ACCESS EXCLUSIVE MODE;
WITH charges AS (
    SELECT user_id, SUM(size_bytes)::BIGINT AS bytes
    FROM user_thumbnail_charge GROUP BY user_id
), reservations AS (
    SELECT user_id, SUM(size_bytes)::BIGINT AS bytes
    FROM thumbnail_upload_reservation GROUP BY user_id
)
UPDATE user_storage_account AS account
SET used_bytes = GREATEST(account.used_bytes - COALESCE((SELECT bytes FROM charges WHERE user_id=account.user_id), 0), 0),
    reserved_bytes = GREATEST(account.reserved_bytes - COALESCE((SELECT bytes FROM reservations WHERE user_id=account.user_id), 0), 0);
DELETE FROM user_thumbnail_charge;
DELETE FROM thumbnail_upload_reservation;
