-- Import commit writes.

-- name: audible_discover_insert!
-- param: content_hash: &str
-- param: policy: &str
-- param: request_json: &str
-- param: updated_at: i64
INSERT INTO audible_enrichment_jobs(content_hash,policy,request_json,status,updated_at)
        VALUES(:content_hash,:policy,:request_json,'pending',:updated_at)
        ON CONFLICT(content_hash) DO UPDATE SET policy=excluded.policy,request_json=excluded.request_json,status='pending',response_json=NULL,
        claim_token=NULL,lease_until=NULL,retry_at=0,updated_at=excluded.updated_at
        WHERE audible_enrichment_jobs.policy<>excluded.policy OR audible_enrichment_jobs.request_json<>excluded.request_json;

-- name: audiobook_applied_request_select?
-- param: content_hash: &str
SELECT request_json FROM audible_enrichment_jobs WHERE content_hash=:content_hash AND status='applied';

-- name: batch_flag_flush&
INSERT INTO sync_outbox(state_kind,state_key,body,changed_at,conflict_rank)
            SELECT 'metadata',b.content_hash,
              bokheim_mutation('metadata',b.content_hash,COALESCE(b.title,''),contributors_for_book(b.row_id),b.book_metadata),
              CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
            FROM pending_metadata_payload p JOIN book b ON b.row_id=p.book_row_id WHERE 1
            ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
              replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
              body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
            DELETE FROM pending_metadata_payload;

-- name: batch_flag_set?
SELECT metadata_batch_flag(1);

-- name: enqueue_asset_work_insert!
-- param: content_hash: &str
INSERT INTO asset_work(content_hash) VALUES(:content_hash) ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;

-- name: enqueue_version_delete!
-- param: content_hash: &str
DELETE FROM rejected_asset_upload WHERE kind='book' AND hash=:content_hash;

-- name: enqueue_version_insert!
-- param: content_hash: &str
-- param: checksum: &str
-- param: size_bytes: i64
INSERT INTO local_book_upload(content_hash,checksum,size_bytes) VALUES(:content_hash,:checksum,:size_bytes)
        ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id,checksum=excluded.checksum,size_bytes=excluded.size_bytes;

-- name: existing_book_contributor_credit?
-- param: content_hash: &str
-- param: position: i64
SELECT author_identity.id, author_identity.stable_id, book_contributor.name
FROM book_contributor JOIN author_identity ON author_identity.id=book_contributor.author_identity_id
WHERE book_contributor.book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND book_contributor.position=:position;

-- name: has_import_inspection_select?
-- param: content_hash: &str
-- param: format: &str
-- param: version: i64
SELECT EXISTS(
    SELECT 1 FROM local_import_inspection i JOIN book b USING(content_hash)
    WHERE i.content_hash = :content_hash AND i.format = :format AND i.version = :version
      AND b.format = :format AND b.deleted_at IS NULL
);

-- name: insert_author_identity!
-- param: stable_id: &str
-- param: preferred_name: &str
-- param: normalized_name: &str
-- param: created_at: i64
INSERT INTO author_identity(stable_id, preferred_name, normalized_name, is_provisional, created_at)
VALUES(:stable_id, :preferred_name, :normalized_name, 1, :created_at);

-- name: insert_book_dir!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
-- param: local_hash: &str
-- param: last_scan: i64
-- param: is_downloaded: i64
INSERT INTO book_dir (dir_id, book_row_id, file_name, local_hash, last_scan, is_downloaded, deleted_at)
VALUES (:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,:local_hash,:last_scan,:is_downloaded,NULL);

-- name: insert_publisher!
-- param: name: &str
-- param: normalized_name: &str
INSERT OR IGNORE INTO publisher(name,normalized_name) VALUES(:name,:normalized_name);

-- name: live_destination_select?
-- param: dir_id: &str
SELECT EXISTS(SELECT 1 FROM dir_paths WHERE id=:dir_id);

-- name: mark_description_scanned!
-- param: content_hash: &str
UPDATE book SET description_scanned = 1
WHERE content_hash = :content_hash AND description_scanned IS NOT 1;

-- name: observe_insert!
-- param: content_hash: &str
-- param: checksum: &str
-- param: size_bytes: i64
-- param: origin: &str
INSERT INTO local_book_versions(content_hash,checksum,size_bytes,origin) VALUES(:content_hash,:checksum,:size_bytes,:origin) ON CONFLICT(content_hash,checksum) DO NOTHING;

-- name: observe_origin_select?
-- param: content_hash: &str
-- param: checksum: &str
SELECT origin FROM local_book_versions WHERE content_hash=:content_hash AND checksum=:checksum;

-- name: observe_book_select?
-- param: dir_id: &str
-- param: content_hash: &str
SELECT checksum FROM local_book_current WHERE dir_id=:dir_id AND content_hash=:content_hash;

-- name: provisional_author_identity_by_name?
-- param: normalized_name: &str
SELECT id, stable_id
FROM author_identity
WHERE normalized_name=:normalized_name AND is_provisional=1
ORDER BY id LIMIT 1;

-- name: publish_conflict_select?
-- param: dir_id: &str
-- param: file_name_key: &str
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.dir_id=:dir_id AND portable_name_key(bd.file_name)=:file_name_key AND b.content_hash!=:content_hash AND bd.deleted_at IS NULL);

-- name: publish_insert!
-- param: content_hash: &str
-- param: dir_id: &str
-- param: relative_path: &str
INSERT INTO local_file_projection(content_hash,dir_id,relative_path) VALUES(:content_hash,:dir_id,:relative_path) ON CONFLICT(content_hash,dir_id) DO UPDATE SET relative_path=excluded.relative_path;

-- name: publish_update_2!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: local_hash: &str
UPDATE book_dir SET local_hash=:local_hash WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: queue_file_work_insert!
-- param: operation: &str
-- param: content_hash: &str
-- param: relative_path: &str
INSERT OR IGNORE INTO local_file_work(operation,content_hash,relative_path) VALUES(:operation,:content_hash,:relative_path);

-- name: record_import_inspection!
-- param: content_hash: &str
-- param: format: &str
-- param: version: i64
INSERT INTO local_import_inspection(content_hash, format, version)
VALUES (:content_hash, :format, :version)
ON CONFLICT(content_hash) DO UPDATE SET format = excluded.format, version = excluded.version;

-- name: remember_placement_insert!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: checksum: &str
-- param: origin: &str
INSERT INTO local_book_current(dir_id,content_hash,checksum,origin) VALUES(:dir_id,:content_hash,:checksum,:origin)
        ON CONFLICT(dir_id,content_hash) DO UPDATE SET id=excluded.id,checksum=excluded.checksum,origin=excluded.origin;

-- name: request_thumbnail!
-- param: content_hash: &str
INSERT INTO local_thumbnail_work(content_hash, state) VALUES (:content_hash, 'pending')
ON CONFLICT(content_hash) DO UPDATE SET state = 'pending', retry_after = 0;

-- name: restore_candidates_select?
-- param: content_hash: &str
SELECT '/' || CASE WHEN dp.path='' THEN bd.file_name ELSE dp.path || '/' || bd.file_name END,p.relative_path
             FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id
             JOIN dir_paths dp ON dp.id=bd.dir_id
             LEFT JOIN local_file_projection p ON p.content_hash=b.content_hash AND p.dir_id=bd.dir_id
             WHERE b.content_hash=:content_hash AND b.deleted_at IS NULL AND bd.deleted_at IS NULL;

-- name: scan_seq_select?
SELECT CAST(scan_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: scan_seq_update!
-- param: scan_seq: i64
INSERT INTO sync_metadata(singleton, scan_seq) VALUES (1, :scan_seq) ON CONFLICT(singleton) DO UPDATE SET scan_seq = excluded.scan_seq;

-- name: scanned_book_exists_select?
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=:content_hash);

-- name: scanned_fold_lifecycle_update!
-- param: content_hash: &str
UPDATE sync_outbox
             SET body=(SELECT bokheim_mutation('book_lifecycle', book.content_hash, 0,
                         COALESCE(book.title, book.content_hash), contributors_for_book(book.row_id),
                         book.book_metadata,
                         COALESCE(book.added_at, 0), book.format, NULL)
                       FROM book WHERE book.content_hash=:content_hash)
             WHERE state_kind='book_lifecycle' AND state_key=:content_hash;

-- name: scanned_fold_metadata_delete!
-- param: content_hash: &str
DELETE FROM sync_outbox WHERE state_kind='metadata' AND state_key=:content_hash;

-- name: set_description!
-- param: content_hash: &str
-- param: description: &str
INSERT INTO book_description(book_row_id, description)
SELECT row_id, :description FROM book WHERE content_hash = :content_hash
  AND (:description <> '' OR EXISTS(SELECT 1 FROM book_description WHERE book_row_id = book.row_id))
ON CONFLICT(book_row_id) DO UPDATE SET description = excluded.description
WHERE book_description.description IS NOT excluded.description;

-- name: upsert_scanned_book!
-- param: content_hash: &str
-- param: title: &str
-- param: book_metadata: &[u8]
-- param: added_at: i64
-- param: format: &str
INSERT INTO book(content_hash,title,description_scanned,book_metadata,added_at,format,deleted_at,purged_at)
VALUES(:content_hash,:title,1,:book_metadata,:added_at,:format,NULL,NULL)
ON CONFLICT(content_hash) DO UPDATE SET
    title=excluded.title,
    format=excluded.format,
    description_scanned=1,
    book_metadata=excluded.book_metadata,
    -- Metadata backfills must not make an existing book look newly added.
    added_at=COALESCE(book.added_at, excluded.added_at),
    deleted_at=NULL,
    purged_at=NULL;
