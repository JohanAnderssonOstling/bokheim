-- Merge mutable registers and schedule exact publications and recovery.

-- name: registers_mark_book_dirty !
-- param: content_hash: &SqlParam<'_>
INSERT OR IGNORE INTO sync_projection_dirty VALUES('book_projection',:content_hash,'');

-- name: registers_store !
-- param: state_kind: &SqlParam<'_>
-- param: state_key: &SqlParam<'_>
-- param: state_subkey: &SqlParam<'_>
-- param: changed_at: &SqlParam<'_>
-- param: conflict_rank: &SqlParam<'_>
-- param: replica_id: &SqlParam<'_>
-- param: replica_seq: &SqlParam<'_>
-- param: body: &SqlParam<'_>
-- param: book_key: &SqlParam<'_>
INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,body,book_key)
         VALUES(:state_kind,:state_key,:state_subkey,:changed_at,:conflict_rank,:replica_id,:replica_seq,:body,:book_key)
         ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank,
         replica_id=excluded.replica_id,replica_seq=excluded.replica_seq,body=excluded.body,book_key=excluded.book_key;

-- name: registers_mark_projection_dirty !
-- param: state_kind: &SqlParam<'_>
-- param: state_key: &SqlParam<'_>
-- param: state_subkey: &SqlParam<'_>
INSERT OR IGNORE INTO sync_projection_dirty VALUES(:state_kind,:state_key,:state_subkey);

-- name: registers_queued ->
-- param: state_kind: &SqlParam<'_>
-- param: state_key: &SqlParam<'_>
-- param: state_subkey: &SqlParam<'_>
SELECT EXISTS(SELECT 1 FROM sync_outbox WHERE state_kind=:state_kind AND state_key=:state_key AND state_subkey=:state_subkey);

-- name: registers_allocate_sequence ->
UPDATE sync_clock SET sequence=sequence+1 WHERE singleton=1 AND sequence<9223372036854775807 RETURNING sequence;

-- name: registers_allocate_batch ->
-- param: count: i64
UPDATE sync_clock SET sequence=sequence+:count
WHERE singleton=1 AND sequence<=9223372036854775807-:count
RETURNING sequence,(SELECT replica_id FROM sync_metadata WHERE singleton=1);

-- name: registers_content_inputs?
-- param: hashes: &str
SELECT requested.key,v.body,v.changed_at,v.conflict_rank,v.replica_id,v.replica_seq,v.book_key
FROM json_each(:hashes) requested
LEFT JOIN sync_state_version v ON v.state_kind='book_content' AND v.state_key=unhex(requested.value) AND v.state_subkey='';

-- name: registers_content_projection_batch!
-- param: publications: &str
INSERT OR IGNORE INTO sync_projection_dirty(state_kind,state_key,state_subkey)
SELECT 'book_content',unhex(json_extract(value,'$[0]')),'' FROM json_each(:publications);

-- name: registers_books_projection_batch!
-- param: hashes: &str
INSERT OR IGNORE INTO sync_projection_dirty(state_kind,state_key,state_subkey)
SELECT 'book_projection',unhex(value),'' FROM json_each(:hashes);

-- name: registers_content_schedule_batch!
-- param: publications: &str
INSERT INTO sync_outbox(replica_seq,state_kind,state_key,state_subkey)
SELECT json_extract(value,'$[1]'),'book_content',unhex(json_extract(value,'$[0]')),'' FROM json_each(:publications)
WHERE true
ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET replica_seq=excluded.replica_seq,body=NULL,changed_at=NULL,winning_version=NULL;

-- name: registers_replica_id ->
SELECT replica_id FROM sync_metadata WHERE singleton=1;

-- name: registers_schedule_publication !
-- param: replica_seq: &SqlParam<'_>
-- param: state_kind: &SqlParam<'_>
-- param: state_key: &SqlParam<'_>
-- param: state_subkey: &SqlParam<'_>
INSERT INTO sync_outbox(replica_seq,state_kind,state_key,state_subkey) VALUES(:replica_seq,:state_kind,:state_key,:state_subkey)
      ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET replica_seq=excluded.replica_seq,body=NULL,changed_at=NULL,winning_version=NULL;

-- name: registers_book_fields ?
-- param: book_key: &SqlParam<'_>
SELECT state_kind,state_key,state_subkey FROM sync_state_version WHERE book_key=:book_key AND state_kind!='book_lifecycle' AND body IS NOT NULL UNION ALL SELECT state_kind,content_hash,'' FROM sync_book_record WHERE content_hash=:book_key;

-- name: registers_canonical_body ->
-- param: state_kind: &SqlParam<'_>
-- param: state_key: &SqlParam<'_>
-- param: state_subkey: &SqlParam<'_>
-- Version columns let publication snapshots use the same read as their body.
SELECT body,conflict_rank,changed_at,replica_id,replica_seq FROM sync_state_version WHERE state_kind=:state_kind AND state_key=:state_key AND state_subkey=:state_subkey AND body IS NOT NULL;
