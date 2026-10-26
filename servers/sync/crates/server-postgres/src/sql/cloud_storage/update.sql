UPDATE libraries SET cloud_storage_enabled=$3,
storage_configuration=CASE WHEN storage_configuration ? 'external' THEN storage_configuration ELSE to_jsonb(CASE WHEN $3 THEN 'bokheim'::text ELSE 'local'::text END) END
WHERE id=$1 AND user_id=$2;
