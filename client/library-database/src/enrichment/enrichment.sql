-- Enrichment state snapshots and durable result accounting.

-- Audible lookup selection and claims. The caller performs the network lookup
-- after receiving a claim; it returns only the resulting response to commit.

-- name: audible_batch_candidates?
-- param: members: &str
-- param: now: i64
-- param: policy: &str
SELECT job.content_hash
FROM json_each(:members) member
JOIN audible_enrichment_jobs job ON job.content_hash = member.value
JOIN book ON book.content_hash = job.content_hash
WHERE job.policy = :policy AND book.deleted_at IS NULL AND book.hidden_at IS NULL
  AND ((job.status IN ('pending', 'unavailable') AND job.retry_at <= :now)
    OR (job.status = 'resolving' AND job.lease_until <= :now))
ORDER BY job.content_hash;

-- name: audible_claim_candidate?
-- param: content_hash: &str
-- param: now: i64
-- param: policy: &str
SELECT job.request_json
FROM audible_enrichment_jobs job
JOIN book ON book.content_hash = job.content_hash
WHERE job.content_hash = :content_hash
  AND job.policy = :policy
  AND book.deleted_at IS NULL AND book.hidden_at IS NULL
  AND ((job.status IN ('pending', 'unavailable') AND job.retry_at <= :now)
    OR (job.status = 'resolving' AND job.lease_until <= :now));

-- name: audible_claim!
-- param: content_hash: &str
-- param: token: &str
-- param: lease_until: i64
-- param: now: i64
UPDATE audible_enrichment_jobs
SET status = 'resolving', claim_token = :token, lease_until = :lease_until, updated_at = :now
WHERE content_hash = :content_hash;

-- name: audible_resolve!
-- param: content_hash: &str
-- param: token: &str
-- param: status: &str
-- param: response: &str
-- param: retry_at: i64
-- param: now: i64
UPDATE audible_enrichment_jobs
SET status = :status, response_json = :response, retry_at = :retry_at,
    claim_token = NULL, lease_until = NULL, updated_at = :now
WHERE content_hash = :content_hash AND claim_token = :token
  AND status = 'resolving' AND lease_until > :now;

-- name: audible_store_toc!
-- param: content_hash: &str
-- param: entry_count: i64
-- param: toc: &str
INSERT INTO book_toc(content_hash, entry_count, toc_json) VALUES (:content_hash, :entry_count, :toc)
ON CONFLICT(content_hash) DO UPDATE SET entry_count = excluded.entry_count, toc_json = excluded.toc_json;

-- name: audible_book_metadata?
-- param: content_hash: &str
SELECT book_metadata FROM book WHERE content_hash = :content_hash;

-- name: audible_update_book_metadata!
-- param: content_hash: &str
-- param: metadata: &[u8]
UPDATE book SET book_metadata = :metadata
WHERE content_hash = :content_hash AND book_metadata IS NOT :metadata;

-- name: audible_clear_identifiers!
-- param: content_hash: &str
DELETE FROM book_identifier WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash = :content_hash);

-- name: audible_insert_identifier!
-- param: content_hash: &str
-- param: position: i64
-- param: scheme: &str
-- param: value: &str
-- param: canonical_value: Option<&str>
-- param: source: &str
-- param: scope: &str
INSERT INTO book_identifier(book_row_id, position, scheme, value, canonical_value, source, scope)
SELECT row_id, :position, :scheme, :value, :canonical_value, :source, :scope
FROM book WHERE content_hash = :content_hash;

-- name: audible_clear_openlibrary_attempts!
-- param: content_hash: &str
DELETE FROM external_metadata_attempt
WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash = :content_hash)
  AND provider_id GLOB 'openlibrary_*';

-- name: enrichment_record_attempt!
-- param: content_hash: &str
-- param: provider: &str
-- param: identifier: &str
-- param: status: &str
-- param: detail: Option<&str>
INSERT INTO external_metadata_attempt(book_row_id, provider_id, identifier, status, detail, attempted_at)
VALUES ((SELECT row_id FROM book WHERE content_hash = :content_hash), :provider, :identifier, :status, :detail, CAST(strftime('%s', 'now') AS INTEGER) * 1000)
ON CONFLICT(book_row_id, provider_id) DO UPDATE SET
    identifier = excluded.identifier,
    status = excluded.status,
    detail = excluded.detail,
    attempted_at = excluded.attempted_at;

-- name: enrichment_set_missing_description!
-- param: content_hash: &str
-- param: description: &str
INSERT INTO book_description(book_row_id, description)
SELECT b.row_id, :description
FROM book b
WHERE b.content_hash = :content_hash
  AND trim(:description) <> ''
  AND NOT EXISTS(
      SELECT 1 FROM book_description current
      WHERE current.book_row_id = b.row_id AND trim(current.description) <> ''
  )
ON CONFLICT(book_row_id) DO UPDATE SET description = excluded.description
WHERE trim(book_description.description) = '';

-- name: enrichment_mark_description_scanned!
-- param: content_hash: &str
UPDATE book SET description_scanned = 1
WHERE content_hash = :content_hash AND description_scanned IS NOT 1;

-- Thumbnail recovery and remote availability.



-- name: thumbnail_pending_job_count ->
SELECT COUNT(*) FROM local_thumbnail_work WHERE state = 'pending';

-- name: thumbnail_pending_jobs?
-- param: after: &str
SELECT content_hash, retry_after FROM local_thumbnail_work
WHERE state = 'pending' AND content_hash > :after
ORDER BY content_hash LIMIT 32;

-- name: thumbnail_local_format?
-- param: content_hash: &str
SELECT b.format
FROM book b
WHERE b.content_hash = :content_hash
  AND EXISTS(
    SELECT 1 FROM book_dir bd JOIN dir d ON d.id = bd.dir_id
    WHERE bd.book_row_id = b.row_id AND bd.deleted_at IS NULL AND d.deleted_at IS NULL
  );

-- name: thumbnail_record_remote!
-- param: content_hash: &str
INSERT INTO remote_asset(kind, hash) VALUES ('thumbnail', :content_hash)
ON CONFLICT(kind, hash) DO NOTHING;

-- name: thumbnail_forget_remote!
-- param: content_hash: &str
DELETE FROM remote_asset WHERE kind = 'thumbnail' AND hash = :content_hash;

-- name: thumbnail_has_remote ->
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM remote_asset WHERE kind = 'thumbnail' AND hash = :content_hash);

-- name: thumbnail_confirm_source!
-- param: content_hash: &str
-- param: origin: &str
INSERT INTO local_thumbnail_source(content_hash, origin) VALUES (:content_hash, :origin)
ON CONFLICT(content_hash) DO UPDATE SET origin = excluded.origin;

-- name: thumbnail_set_result!
-- param: content_hash: &str
-- param: state: &str
INSERT INTO local_thumbnail_work(content_hash, state) VALUES (:content_hash, :state)
ON CONFLICT(content_hash) DO UPDATE SET state = excluded.state, retry_after = 0;

-- name: thumbnail_retry!
-- param: content_hash: &str
-- param: retry_after: i64
INSERT INTO local_thumbnail_work(content_hash, state, retry_after) VALUES (:content_hash, 'pending', :retry_after)
ON CONFLICT(content_hash) DO UPDATE SET state = 'pending', retry_after = excluded.retry_after;

-- name: thumbnail_discard!
-- param: content_hash: &str
DELETE FROM local_thumbnail_work WHERE content_hash = :content_hash;

-- name: thumbnail_request_missing!
-- param: content_hash: &str
INSERT INTO local_thumbnail_work(content_hash, state) VALUES (:content_hash, 'pending')
ON CONFLICT(content_hash) DO UPDATE SET state = 'pending', retry_after = 0
WHERE local_thumbnail_work.state = 'ready';

-- name: thumbnail_work_state?
-- param: content_hash: &str
SELECT state, retry_after FROM local_thumbnail_work WHERE content_hash = :content_hash;

-- name: thumbnail_enqueue_asset_work!
-- param: content_hash: &str
INSERT INTO asset_work(content_hash) VALUES (:content_hash)
ON CONFLICT(content_hash) DO UPDATE SET id = excluded.id;

-- name: thumbnail_upload_allowed ->
-- param: content_hash: &str
SELECT upload.id IS NOT NULL
   AND rejected.hash IS NULL
   AND remote.hash IS NULL
   AND COALESCE(work.state = 'ready', 0)
   AND EXISTS(SELECT 1 FROM local_thumbnail_source source WHERE source.content_hash = book.content_hash AND source.origin = 'local')
FROM book
LEFT JOIN local_book_upload upload ON upload.content_hash = book.content_hash
LEFT JOIN remote_asset remote ON remote.kind = 'thumbnail' AND remote.hash = book.content_hash
LEFT JOIN rejected_asset_upload rejected ON rejected.kind = 'thumbnail' AND rejected.hash = book.content_hash AND rejected.rejected_at > unixepoch() - 300
LEFT JOIN local_thumbnail_work work ON work.content_hash = book.content_hash
WHERE book.content_hash = :content_hash AND book.deleted_at IS NULL;

-- Subject and external metadata selection.
-- name: enrichment_subject_candidates?
-- param: after: &str
-- param: scope: &str
SELECT b.content_hash,COALESCE(b.title,''),b.format,b.book_metadata,CASE WHEN s.metadata=b.book_metadata AND s.title=COALESCE(b.title,'') THEN s.state ELSE NULL END
FROM book b LEFT JOIN local_subject_pipeline s USING(content_hash)
WHERE b.content_hash>:after AND b.deleted_at IS NULL AND b.hidden_at IS NULL AND b.content_hash IN (SELECT value FROM json_each(:scope))
AND NOT EXISTS(SELECT 1 FROM pending_audible_enrichment a WHERE a.content_hash=b.content_hash)
AND (s.content_hash IS NULL OR s.metadata<>b.book_metadata OR s.title<>COALESCE(b.title,'') OR s.complete=0 OR s.retry_at>0)
ORDER BY b.content_hash LIMIT 32;

-- name: enrichment_book?
-- param: content_hash: &str
SELECT book_metadata FROM book WHERE content_hash=:content_hash;

-- name: enrichment_step_succeeded?
-- param: content_hash: &str
-- param: provider: &str
SELECT status <> 'failed' FROM external_metadata_attempt WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND provider_id=:provider;

-- name: enrichment_cached_isbn?
-- param: isbn: &str
SELECT response FROM local_isbn_response WHERE isbn=:isbn AND expires_at>unixepoch();

-- name: enrichment_thumbnail_state?
-- param: content_hash: &str
SELECT state,retry_after FROM local_thumbnail_work WHERE content_hash=:content_hash;

-- name: enrichment_book_format?
-- param: content_hash: &str
SELECT format FROM book WHERE content_hash=:content_hash AND deleted_at IS NULL;

-- name: enrichment_description_candidates?
-- param: provider: &str
-- param: after: &str
-- param: scope: &str
WITH lookup_isbn AS (SELECT book_row_id,canonical_value FROM book_identifier WHERE scheme='isbn' AND canonical_value IS NOT NULL), candidates AS (
 SELECT b.row_id,b.content_hash,group_concat(DISTINCT i.canonical_value ORDER BY i.canonical_value) isbns FROM book b JOIN lookup_isbn i ON i.book_row_id=b.row_id
 WHERE b.deleted_at IS NULL AND b.hidden_at IS NULL AND trim(COALESCE((SELECT description FROM book_description WHERE book_row_id=b.row_id),''))='' AND b.content_hash>:after AND b.content_hash IN (SELECT value FROM json_each(:scope)) AND NOT EXISTS(SELECT 1 FROM pending_audible_enrichment a WHERE a.content_hash=b.content_hash) GROUP BY b.row_id)
SELECT c.content_hash,c.isbns FROM candidates c LEFT JOIN external_metadata_attempt a ON a.book_row_id=c.row_id AND a.provider_id=:provider
WHERE a.book_row_id IS NULL OR a.identifier<>c.isbns OR a.status='updated' OR a.attempted_at<unixepoch()*1000-2592000000 OR (a.status='failed' AND a.attempted_at<unixepoch()*1000-60000) ORDER BY c.content_hash LIMIT 32;

-- name: enrichment_cover_candidates?
-- param: provider: &str
-- param: after: &str
-- param: scope: &str
WITH candidates AS (
 SELECT b.row_id,b.content_hash,w.state,(SELECT group_concat(canonical_value,',') FROM (SELECT DISTINCT canonical_value FROM book_identifier WHERE book_row_id=b.row_id AND scheme='isbn' AND canonical_value IS NOT NULL ORDER BY canonical_value)) isbns FROM book b JOIN local_thumbnail_work w ON w.content_hash=b.content_hash
 WHERE b.content_hash>:after AND b.content_hash IN (SELECT value FROM json_each(:scope)) AND (w.state='no_cover' OR (w.state='ready' AND lower(b.format)='m4b')) AND b.deleted_at IS NULL AND b.hidden_at IS NULL AND NOT EXISTS(SELECT 1 FROM pending_audible_enrichment a WHERE a.content_hash=b.content_hash))
SELECT c.content_hash,c.isbns FROM candidates c LEFT JOIN external_metadata_attempt a ON a.book_row_id=c.row_id AND a.provider_id=:provider WHERE c.isbns IS NOT NULL AND (a.book_row_id IS NULL OR a.identifier<>c.isbns OR (a.status='updated' AND c.state='no_cover') OR (a.status='failed' AND a.attempted_at<=unixepoch()*1000-30000)) ORDER BY c.content_hash LIMIT 32;

-- name: enrichment_author_candidates?
-- param: provider: &str
-- param: after: &str
-- param: scope: &str
WITH lookup_isbn AS (SELECT book_row_id,canonical_value FROM book_identifier WHERE scheme='isbn' AND canonical_value IS NOT NULL)
SELECT b.content_hash,group_concat(DISTINCT i.canonical_value) FROM book b JOIN lookup_isbn i ON i.book_row_id=b.row_id
WHERE b.content_hash>:after AND b.content_hash IN (SELECT value FROM json_each(:scope)) AND b.deleted_at IS NULL AND b.hidden_at IS NULL AND NOT EXISTS(SELECT 1 FROM pending_audible_enrichment a WHERE a.content_hash=b.content_hash)
AND NOT EXISTS(SELECT 1 FROM external_metadata_attempt attempt WHERE attempt.book_row_id=b.row_id AND attempt.provider_id=:provider AND attempt.attempted_at>=unixepoch()*1000-2592000000 AND attempt.status<>'failed') GROUP BY b.row_id ORDER BY b.content_hash LIMIT 32;

-- name: enrichment_author_credits?
-- param: content_hash: &str
SELECT position,name FROM book_contributor WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND role=X'617574' ORDER BY position;

-- name: enrichment_identity_input?
-- param: content_hash: &str
SELECT COALESCE(title,''),book_metadata FROM book WHERE content_hash=:content_hash AND deleted_at IS NULL AND hidden_at IS NULL;

-- name: enrichment_identity_authors?
-- param: content_hash: &str
SELECT name FROM book_contributor WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND role=X'617574' ORDER BY position;

-- name: enrichment_identity_filenames?
-- param: content_hash: &str
SELECT file_name FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND deleted_at IS NULL ORDER BY file_name;

-- name: upload_preparation_snapshot?
SELECT b.content_hash, u.id
FROM book b LEFT JOIN local_book_upload u ON u.content_hash = b.content_hash
WHERE b.deleted_at IS NULL
ORDER BY b.content_hash;

-- name: prepared_upload_batch?
-- param: scope: &str
WITH RECURSIVE affected_dirs(id) AS (
    SELECT dir_id FROM local_directory_work
    UNION SELECT d.id FROM dir d JOIN affected_dirs a ON d.parent_id = a.id WHERE d.id != d.parent_id
)
SELECT b.content_hash, u.id
FROM json_each(:scope) members JOIN book b ON b.content_hash = members.value
LEFT JOIN local_book_upload u ON u.content_hash = b.content_hash
WHERE b.deleted_at IS NULL
  AND NOT EXISTS(SELECT 1 FROM local_book_work w WHERE w.content_hash = b.content_hash)
  AND NOT EXISTS(SELECT 1 FROM local_file_work w WHERE w.content_hash = b.content_hash AND w.operation IN ('restore', 'trash'))
  AND NOT EXISTS(SELECT 1 FROM book_dir bd JOIN affected_dirs d ON d.id = bd.dir_id WHERE bd.book_row_id = b.row_id)
  AND NOT EXISTS(SELECT 1 FROM local_file_projection fp JOIN affected_dirs d ON d.id = fp.dir_id WHERE fp.content_hash = b.content_hash)
ORDER BY b.content_hash;

-- name: upload_thumbnail_candidates?
-- param: scope: &str
-- param: now: i64
SELECT w.content_hash
FROM json_each(:scope) members
JOIN local_thumbnail_work w ON w.content_hash = members.value
WHERE w.state = 'pending' AND w.retry_after <= :now
ORDER BY w.content_hash;

-- name: finish_upload_preparation!
-- param: members: &str
-- param: policy: &str
INSERT INTO local_upload_preparation(content_hash, upload_id)
SELECT b.content_hash, json_extract(member.value, '$[1]')
FROM json_each(:members) member
JOIN book b ON b.content_hash = json_extract(member.value, '$[0]')
WHERE b.deleted_at IS NULL
  AND NOT EXISTS(
      SELECT 1 FROM audible_enrichment_jobs j
      WHERE j.content_hash = b.content_hash AND j.policy = :policy AND j.status IN ('pending', 'resolving')
  )
ON CONFLICT(content_hash) DO UPDATE SET upload_id = excluded.upload_id
WHERE local_upload_preparation.upload_id IS NOT excluded.upload_id;

-- name: enrichment_status_counts ->
-- param: edition_provider: &str
-- param: rich_provider: &str
-- param: author_provider: &str
-- param: cover_provider: &str
WITH per_book AS (
    SELECT b.row_id,
        CASE
            WHEN MAX(a.status='updated')=1 OR MAX(j.status='applied')=1 THEN 'enriched'
            WHEN MAX(a.status='failed')=1 OR MAX(j.status='unavailable')=1 THEN 'failed'
            WHEN MAX(a.status='ambiguous')=1 OR MAX(j.status='ambiguous')=1 THEN 'no_match'
            WHEN MAX(pending.content_hash IS NOT NULL)=1 THEN 'pending'
            WHEN COUNT(a.provider_id)>0 OR MAX(j.status='no_match')=1 THEN 'no_match'
            ELSE 'pending'
        END AS state
    FROM book b LEFT JOIN external_metadata_attempt a
        ON a.book_row_id=b.row_id AND a.provider_id IN (:edition_provider,:rich_provider,:author_provider,:cover_provider)
    LEFT JOIN audible_enrichment_jobs j ON j.content_hash=b.content_hash
    LEFT JOIN pending_audible_enrichment pending ON pending.content_hash=b.content_hash
    WHERE b.deleted_at IS NULL AND b.hidden_at IS NULL
    GROUP BY b.row_id
) SELECT COUNT(*), COALESCE(SUM(state='enriched'),0), COALESCE(SUM(state='pending'),0),
    COALESCE(SUM(state='no_match'),0), COALESCE(SUM(state='failed'),0)
  FROM per_book;

-- name: enrichment_has_import_inspection ->
-- param: content_hash: &str
-- param: format: &str
-- param: version: i64
SELECT EXISTS(
    SELECT 1 FROM local_import_inspection i JOIN book b USING(content_hash)
    WHERE i.content_hash = :content_hash AND i.format = :format AND i.version = :version
      AND b.format = :format AND b.deleted_at IS NULL
);

-- name: schema_external_metadata &
-- External metadata attempts and the entities they resolve to.
create table if not exists external_metadata_attempt(
    book_row_id     INTEGER not null references book(row_id) on delete cascade,
    provider_id     TEXT not null,
    identifier      TEXT not null,
    status          TEXT not null check(status in ('updated', 'no_match', 'ambiguous', 'failed')),
    detail          TEXT,
    attempted_at    INTEGER not null,
    primary key(book_row_id, provider_id)
);
create index if not exists idx_external_metadata_attempt_status on external_metadata_attempt(provider_id, status, book_row_id);
-- Local cache of resolved metadata-server entities. Book references are also
-- projected into synchronized BookMetadata; labels and graph edges can
-- be rebuilt independently on every client from the same snapshot service.
create table if not exists external_entity(
    wikidata_id    TEXT primary key,
    kind           TEXT not null check(kind in ('place', 'person')),
    label          TEXT not null,
    birth_year     INTEGER,
    death_year     INTEGER,
    fictional      INTEGER not null default 0 check(fictional in (0, 1)),
    latitude_e7    INTEGER,
    longitude_e7   INTEGER
);
create table if not exists external_entity_alias(
    wikidata_id    TEXT not null references external_entity(wikidata_id) on delete cascade,
    alias          TEXT not null,
    primary key(wikidata_id, alias)
);
create table if not exists external_place_edge(
    child_id       TEXT not null references external_entity(wikidata_id) on delete cascade,
    parent_id      TEXT not null references external_entity(wikidata_id) on delete cascade,
    relation       TEXT not null check(relation in ('parent', 'country')),
    primary key(child_id, parent_id, relation)
);
create index if not exists idx_external_place_edge_parent on external_place_edge(parent_id, child_id);
create table if not exists book_external_entity(
    book_row_id    INTEGER not null references book(row_id) on delete cascade,
    wikidata_id    TEXT not null references external_entity(wikidata_id) on delete cascade,
    kind           TEXT not null check(kind in ('place', 'person')),
    evidence_name  TEXT not null,
    source         TEXT not null check(source in ('exact_edition', 'work')),
    primary key(book_row_id, wikidata_id, kind, evidence_name)
);
create index if not exists idx_book_external_entity_lookup on book_external_entity(kind, wikidata_id, book_row_id);
create table if not exists book_collection(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    name            TEXT not null,
    collection_type TEXT,
    group_position  TEXT,
    primary key(book_row_id, position)
);
create table if not exists book_date(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    value           TEXT not null,
    normalized_year INTEGER,
    normalized_month INTEGER,
    normalized_day INTEGER,
    event           TEXT,
    primary key(book_row_id, position)
);
create table if not exists book_value(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    kind            TEXT not null,
    position        INTEGER not null check(position >= 0),
    value           TEXT not null,
    qualifier       TEXT,
    secondary_value TEXT,
    primary key(book_row_id, kind, position)
);
create index if not exists idx_book_value_lookup on book_value(kind, value, book_row_id);

-- A library-owned relation from stable catalogue book identities to
-- content-addressed books. Catalogue configuration, URLs, and cached metadata
-- remain application-wide and are deliberately absent here.

-- name: schema_metadata_batch &
-- Metadata coalescing triggers.
-- Version 4: description-independent metadata; connection-local batching mode; explicit upgrade definitions.
CREATE TABLE IF NOT EXISTS pending_metadata_payload(book_row_id INTEGER PRIMARY KEY REFERENCES book(row_id) ON DELETE CASCADE);
CREATE TRIGGER trig_book_metadata
AFTER UPDATE OF title, book_metadata ON book
WHEN (OLD.title IS NOT NEW.title OR OLD.book_metadata IS NOT NEW.book_metadata)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
 AND metadata_batch_flag(NULL)=0
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    VALUES (
        'metadata', NEW.content_hash,
        bokheim_mutation('metadata', NEW.content_hash, COALESCE(NEW.title, ''), contributors_for_book(NEW.row_id), NEW.book_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    )
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER trig_book_metadata_queued
AFTER UPDATE OF title, book_metadata ON book
WHEN (OLD.title IS NOT NEW.title OR OLD.book_metadata IS NOT NEW.book_metadata)
  AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
 AND metadata_batch_flag(NULL)=1
BEGIN
 INSERT INTO pending_metadata_payload SELECT row_id FROM book WHERE row_id=NEW.row_id ON CONFLICT(book_row_id) DO NOTHING;
END;
CREATE TRIGGER trig_book_contributor_insert
AFTER INSERT ON book_contributor
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
 AND metadata_batch_flag(NULL)=0
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    SELECT
        'metadata', book.content_hash,
        bokheim_mutation('metadata', book.content_hash, COALESCE(book.title, ''), contributors_for_book(book.row_id), book.book_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    FROM book WHERE book.row_id = NEW.book_row_id
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER trig_book_contributor_insert_queued
AFTER INSERT ON book_contributor
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
 AND metadata_batch_flag(NULL)=1
BEGIN
 INSERT INTO pending_metadata_payload SELECT row_id FROM book WHERE row_id=NEW.book_row_id ON CONFLICT(book_row_id) DO NOTHING;
END;
CREATE TRIGGER trig_book_contributor_update
AFTER UPDATE ON book_contributor
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
 AND metadata_batch_flag(NULL)=0
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    SELECT
        'metadata', book.content_hash,
        bokheim_mutation('metadata', book.content_hash, COALESCE(book.title, ''), contributors_for_book(book.row_id), book.book_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    FROM book WHERE book.row_id = NEW.book_row_id
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER trig_book_contributor_update_queued
AFTER UPDATE ON book_contributor
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
 AND metadata_batch_flag(NULL)=1
BEGIN
 INSERT INTO pending_metadata_payload SELECT row_id FROM book WHERE row_id=NEW.book_row_id ON CONFLICT(book_row_id) DO NOTHING;
END;
CREATE TRIGGER trig_book_contributor_delete
AFTER DELETE ON book_contributor
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
  AND EXISTS (SELECT 1 FROM book WHERE book.row_id = OLD.book_row_id)
 AND metadata_batch_flag(NULL)=0
BEGIN
    INSERT INTO sync_outbox(state_kind, state_key, body, changed_at, conflict_rank)
    SELECT
        'metadata', book.content_hash,
        bokheim_mutation('metadata', book.content_hash, COALESCE(book.title, ''), contributors_for_book(book.row_id), book.book_metadata),
        CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER), 0
    FROM book WHERE book.row_id = OLD.book_row_id
    ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
        replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
        body=excluded.body, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER trig_book_contributor_delete_queued
AFTER DELETE ON book_contributor
WHEN COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1), 'local') = 'local'
  AND EXISTS (SELECT 1 FROM book WHERE book.row_id = OLD.book_row_id)
 AND metadata_batch_flag(NULL)=1
BEGIN
 INSERT INTO pending_metadata_payload SELECT row_id FROM book WHERE row_id=OLD.book_row_id ON CONFLICT(book_row_id) DO NOTHING;
END;

-- name: request_thumbnail!
-- param: content_hash: &str
INSERT INTO local_thumbnail_work(content_hash, state) VALUES (:content_hash, 'pending')
ON CONFLICT(content_hash) DO UPDATE SET state = 'pending', retry_after = 0;
