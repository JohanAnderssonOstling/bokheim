WITH input AS (
    SELECT *
    FROM UNNEST(
        $2::text[], $3::bytea[], $4::text[], $5::bytea[],
        $6::bytea[], $7::bigint[], $8::bigint[], $9::smallint[], $10::text[]
    ) AS rows(kind, entity_key, entity_subkey, value, content_hash, changed_at, replica_seq, version_rank, replica_id)
), reduced AS (
    SELECT DISTINCT ON (kind, entity_key, entity_subkey) *
    FROM input
    ORDER BY kind, entity_key, entity_subkey, changed_at DESC, version_rank DESC, replica_id COLLATE "C" DESC, replica_seq DESC
)
INSERT INTO sync_state(library_id,kind,entity_key,entity_subkey,value,content_hash,changed_at,replica_id,replica_seq,version_rank)
SELECT $1,kind,entity_key,entity_subkey,value,content_hash,changed_at,replica_id,replica_seq,version_rank
FROM reduced
ON CONFLICT(library_id,kind,entity_key,entity_subkey) DO UPDATE SET
    value=EXCLUDED.value, content_hash=EXCLUDED.content_hash,
    changed_at=EXCLUDED.changed_at, replica_id=EXCLUDED.replica_id, replica_seq=EXCLUDED.replica_seq,
    version_rank=EXCLUDED.version_rank, server_seq=DEFAULT;
