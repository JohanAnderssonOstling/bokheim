-- Store immutable book records and retain their publication tokens.

-- name: records_snapshot ->
-- param: state_kind: &SqlParam<'_>
-- param: content_hash: &SqlParam<'_>
-- param: subkey: &SqlParam<'_>
SELECT body,publication FROM sync_book_record WHERE state_kind=:state_kind AND content_hash=:content_hash AND :subkey='';

-- name: records_publication_version ->
-- param: state_kind: &SqlParam<'_>
-- param: content_hash: &SqlParam<'_>
-- param: subkey: &SqlParam<'_>
SELECT publication FROM sync_book_record WHERE state_kind=:state_kind AND content_hash=:content_hash AND :subkey='';

-- name: records_contains ->
-- param: state_kind: &SqlParam<'_>
-- param: content_hash: &SqlParam<'_>
SELECT EXISTS(SELECT 1 FROM sync_book_record WHERE state_kind=:state_kind AND content_hash=:content_hash);

-- name: records_replica_id ->
SELECT replica_id FROM sync_metadata WHERE singleton=1;

-- name: records_reset_cover_checks_batch!
-- param: publications: &str
UPDATE book_transfer SET cover_checked=0
WHERE cover_checked!=0 AND content_hash IN (SELECT unhex(json_extract(value,'$[0]')) FROM json_each(:publications));

-- name: records_discard_drafts_batch!
-- param: publications: &str
DELETE FROM local_book_draft
WHERE content_hash IN (SELECT unhex(json_extract(value,'$[0]')) FROM json_each(:publications));

-- name: records_mark_projection_dirty_batch!
-- param: publications: &str
INSERT OR IGNORE INTO sync_projection_dirty(state_kind,state_key,state_subkey)
SELECT 'book_add',unhex(json_extract(value,'$[0]')),'' FROM json_each(:publications);

-- name: records_schedule_batch!
-- param: publications: &str
INSERT INTO sync_outbox(replica_seq,state_kind,state_key,state_subkey)
SELECT json_extract(value,'$[1]'),'book_add',unhex(json_extract(value,'$[0]')),'' FROM json_each(:publications)
WHERE true
ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET replica_seq=excluded.replica_seq,body=NULL,changed_at=NULL,winning_version=NULL;

-- name: records_store !
-- param: state_kind: &SqlParam<'_>
-- param: content_hash: &SqlParam<'_>
-- param: body: &SqlParam<'_>
-- param: publication: &SqlParam<'_>
INSERT INTO sync_book_record(state_kind,content_hash,body,publication) VALUES(:state_kind,:content_hash,:body,:publication)
         ON CONFLICT(state_kind,content_hash) DO UPDATE SET body=excluded.body,publication=excluded.publication
         WHERE (body,publication) IS NOT (excluded.body,excluded.publication);

-- name: records_reset_cover_check !
-- param: content_hash: &SqlParam<'_>
UPDATE book_transfer SET cover_checked=0 WHERE content_hash=:content_hash;

-- name: records_mark_projection_dirty !
-- param: state_kind: &SqlParam<'_>
-- param: state_key: &SqlParam<'_>
INSERT OR IGNORE INTO sync_projection_dirty VALUES(:state_kind,:state_key,'');

-- name: records_stage !
-- param: content_hash: &SqlParam<'_>
-- param: body: &SqlParam<'_>
INSERT OR IGNORE INTO local_book_draft(content_hash,body) VALUES(:content_hash,:body);

-- name: records_discard_draft !
-- param: content_hash: &SqlParam<'_>
DELETE FROM local_book_draft WHERE content_hash=:content_hash;
