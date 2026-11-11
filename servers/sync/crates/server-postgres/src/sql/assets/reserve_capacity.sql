UPDATE user_storage_account
SET reserved_bytes = reserved_bytes + $2
WHERE user_id = $1
  AND used_bytes + reserved_bytes + $2 <= billing_storage_quota(user_id, quota_bytes)
RETURNING billing_storage_quota(user_id, quota_bytes) AS quota_bytes, used_bytes, reserved_bytes;
