UPDATE user_storage_account SET used_bytes=used_bytes+$2 WHERE user_id=$1
