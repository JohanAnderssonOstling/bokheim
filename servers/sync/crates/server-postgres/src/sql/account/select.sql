SELECT billing_storage_quota(user_id, quota_bytes) AS quota_bytes, used_bytes, reserved_bytes
FROM user_storage_account
WHERE user_id = $1;
