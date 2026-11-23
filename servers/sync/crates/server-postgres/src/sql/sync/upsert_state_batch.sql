WITH input AS (
    SELECT *
    FROM UNNEST(
        $2::text[], $3::bytea[], $4::text[], $5::bytea[],
        $6::bytea[], $7::bigint[], $8::bigint[], $9::smallint[], $10::text[]
    ) AS rows(kind, entity_key, entity_subkey, value, content_hash, changed_at, replica_seq, version_rank, replica_id)
), reduced AS (
    SELECT DISTINCT ON (input.kind, input.entity_key, input.entity_subkey) input.*
    FROM input
    LEFT JOIN sync_state stored
      ON stored.library_id=$1 AND stored.kind=input.kind
     AND stored.entity_key=input.entity_key AND stored.entity_subkey=input.entity_subkey
    ORDER BY input.kind, input.entity_key, input.entity_subkey,
        -- An exact retry must not hide another publication that needs the
        -- stored winner redelivered. The trigger still decides what wins.
        ((input.replica_id, input.replica_seq) IS DISTINCT FROM (stored.replica_id, stored.replica_seq)) DESC,
        input.changed_at DESC, input.version_rank DESC,
        input.replica_id COLLATE "C" DESC, input.replica_seq DESC
)
INSERT INTO sync_state(library_id,kind,entity_key,entity_subkey,value,content_hash,changed_at,replica_id,replica_seq,version_rank)
SELECT $1,kind,entity_key,entity_subkey,value,content_hash,changed_at,replica_id,replica_seq,version_rank
FROM reduced
ON CONFLICT(library_id,kind,entity_key,entity_subkey) DO UPDATE SET
    value=EXCLUDED.value, content_hash=EXCLUDED.content_hash,
    changed_at=EXCLUDED.changed_at, replica_id=EXCLUDED.replica_id, replica_seq=EXCLUDED.replica_seq,
    version_rank=EXCLUDED.version_rank, server_seq=DEFAULT;
