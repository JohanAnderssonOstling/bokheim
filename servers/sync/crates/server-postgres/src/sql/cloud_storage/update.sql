UPDATE libraries SET cloud_storage_enabled=$3,
storage_configuration=to_jsonb(CASE WHEN $3 THEN 'bokheim'::text ELSE 'local'::text END)
WHERE id=$1 AND user_id=$2;
