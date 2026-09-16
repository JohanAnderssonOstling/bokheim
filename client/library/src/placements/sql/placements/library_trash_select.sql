SELECT content_hash,COALESCE(NULLIF(title,''),content_hash),format,deleted_at
         FROM book WHERE deleted_at IS NOT NULL AND purged_at IS NULL
         ORDER BY deleted_at DESC,portable_name_key(COALESCE(title,content_hash)),content_hash