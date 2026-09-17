-- Sync apply: remote change projection.



-- name: activate_remote_directory!
-- param: dir_id: &str
UPDATE dir SET deleted_at=NULL, purged_at=NULL WHERE id=:dir_id;

-- name: apply_insert!
-- param: system_id: &str
-- param: version: &str
-- param: code: &str
INSERT INTO source_subject(system_id,source_version,code,official_label) VALUES(:system_id,:version,:code,NULL) ON CONFLICT DO NOTHING;

-- name: apply_insert_2!
-- param: system_id: &str
-- param: version: &str
-- param: code: &str
-- param: concept_id: i64
-- param: mapping_type: &str
-- param: mapper_version: i64
INSERT INTO source_concept_mapping(source_system_id,source_version,source_code,concept_id,mapping_type,mapper_version) VALUES(:system_id,:version,:code,:concept_id,:mapping_type,:mapper_version)
            ON CONFLICT(source_system_id,source_version,source_code,concept_id) DO UPDATE SET mapping_type=excluded.mapping_type,mapper_version=excluded.mapper_version
            WHERE mapping_type IS NOT excluded.mapping_type OR mapper_version IS NOT excluded.mapper_version;

-- name: apply_insert_3!
-- param: system_id: &str
-- param: code: &str
-- param: unified_path: &str
-- param: mapper_version: i64
INSERT INTO subject_code_mapping(source_system_id,source_code,unified_path,mapper_version) VALUES(:system_id,:code,:unified_path,:mapper_version)
            ON CONFLICT(source_system_id,source_code,unified_path) DO UPDATE SET mapper_version=excluded.mapper_version WHERE mapper_version IS NOT excluded.mapper_version;

-- name: apply_remote_book_added!
-- param: content_hash: &str
-- param: title: &str
-- param: added_at: i64
-- param: format: &str
INSERT INTO book (content_hash, title, read_pos, read_progress, added_at, format, deleted_at, purged_at)
VALUES (:content_hash, :title, NULL, 0, :added_at, :format, NULL, NULL)
ON CONFLICT(content_hash) DO UPDATE SET
    title = COALESCE(book.title, excluded.title),
    added_at = excluded.added_at,
    format = excluded.format,
    deleted_at = NULL,
    purged_at = NULL,
    trash_origin_dir_id = NULL;

-- name: apply_remote_book_deleted!
-- param: content_hash: &str
-- param: changed_at: i64
-- param: origin_folder_id: Option<&str>
INSERT INTO book (content_hash, added_at, deleted_at, trash_origin_dir_id)
VALUES (:content_hash, :changed_at, :changed_at, :origin_folder_id)
ON CONFLICT(content_hash) DO UPDATE SET
    deleted_at = excluded.deleted_at,
    purged_at = NULL,
    trash_origin_dir_id = :origin_folder_id;

-- name: apply_remote_book_dir_added!
-- param: dir_id: &str
-- param: content_hash: &str
INSERT INTO book_dir (dir_id, book_row_id, file_name, local_hash, last_scan, is_downloaded, deleted_at)
VALUES (
    :dir_id,
    (SELECT row_id FROM book WHERE content_hash=:content_hash),
    COALESCE((SELECT file_name FROM book_dir WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash)), :content_hash),
    '',
    0,
    0,
    NULL
)
ON CONFLICT(dir_id, book_row_id) DO UPDATE SET
    deleted_at = NULL,
    trash_origin_dir_id = NULL,
    local_hash = excluded.local_hash,
    is_downloaded = max(book_dir.is_downloaded, excluded.is_downloaded);

-- name: apply_remote_book_dir_removed!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: origin_folder_id: Option<&str>
INSERT INTO book_dir (dir_id, book_row_id, file_name, local_hash, last_scan, is_downloaded, deleted_at, trash_origin_dir_id)
VALUES (
    :dir_id,
    (SELECT row_id FROM book WHERE content_hash=:content_hash),
    COALESCE((SELECT file_name FROM book_dir WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash)), :content_hash),
    '',
    0,
    0,
    1,
    :origin_folder_id
)
ON CONFLICT(dir_id, book_row_id) DO UPDATE SET
    deleted_at = 1,
    trash_origin_dir_id = excluded.trash_origin_dir_id;

-- name: apply_remote_book_purged!
-- param: content_hash: &str
-- param: changed_at: i64
INSERT INTO book (content_hash, added_at, deleted_at, purged_at)
VALUES (:content_hash, :changed_at, :changed_at, :changed_at)
ON CONFLICT(content_hash) DO UPDATE SET
    deleted_at = excluded.deleted_at,
    purged_at = excluded.purged_at,
    trash_origin_dir_id = NULL;

-- name: apply_remote_book_purged_delete!
-- param: book_row_id: i64
DELETE FROM book_dir WHERE book_row_id=:book_row_id;

-- name: apply_remote_book_purged_delete_2!
-- param: content_hash: &str
DELETE FROM audiobook_metadata WHERE content_hash=:content_hash;

-- name: apply_remote_book_purged_delete_3!
-- param: book_row_id: i64
DELETE FROM book_description WHERE book_row_id=:book_row_id;

-- name: apply_remote_book_purged_select?
-- param: content_hash: &str
SELECT row_id FROM book WHERE content_hash=:content_hash;

-- name: apply_remote_book_purged_update!
-- param: book_row_id: i64
UPDATE book SET title=NULL,description_scanned=0,book_metadata=X'',format='',hidden_at=NULL,read_pos=NULL,read_progress=0,added_at=NULL,trash_origin_dir_id=NULL WHERE row_id=:book_row_id;

-- name: apply_remote_books_added_batch!
-- param: books_json: &str
INSERT INTO book (content_hash, title, read_pos, read_progress, added_at, format, deleted_at, purged_at)
SELECT json_extract(value,'$[0]'), json_extract(value,'$[1]'), NULL, 0,
       json_extract(value,'$[2]'), json_extract(value,'$[3]'), NULL, NULL
FROM json_each(:books_json) WHERE true ORDER BY CAST(key AS INTEGER)
ON CONFLICT(content_hash) DO UPDATE SET
    title = COALESCE(book.title, excluded.title),
    added_at = excluded.added_at,
    format = excluded.format,
    deleted_at = NULL,
    purged_at = NULL,
    trash_origin_dir_id = NULL;

-- name: apply_remote_change_inner_insert!
-- param: content_hash: &str
INSERT OR IGNORE INTO local_purge_work(content_hash) VALUES(:content_hash);

-- name: author_identity_by_stable_id?
-- param: stable_id: &str
SELECT id, stable_id FROM author_identity WHERE stable_id=:stable_id;

-- name: authors_contributors_match_select?
-- param: content_hash: &str
SELECT c.position,a.stable_id,c.name,c.sort_name,c.role
        FROM book_contributor c LEFT JOIN author_identity a ON a.id=c.author_identity_id
        WHERE c.book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) ORDER BY c.position;

-- name: authors_insert_author_identity!
-- param: stable_id: &str
-- param: preferred_name: &str
-- param: normalized_name: &str
-- param: created_at: i64
INSERT INTO author_identity(stable_id, preferred_name, normalized_name, is_provisional, created_at)
VALUES(:stable_id, :preferred_name, :normalized_name, 1, :created_at);

-- name: authors_publisher_select?
-- param: normalized_name: &str
SELECT id FROM publisher WHERE normalized_name=:normalized_name;

-- name: authors_trim_book_contributors!
-- param: content_hash: &str
-- param: position: i64
DELETE FROM book_contributor WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND position>=:position;

-- name: authors_upsert_book_contributor!
-- param: content_hash: &str
-- param: author_identity_id: i64
-- param: position: i64
-- param: name: &str
-- param: sort_key: Option<&str>
-- param: role: &[u8]
INSERT INTO book_contributor(book_row_id, author_identity_id, position, name, sort_name, role)
VALUES((SELECT row_id FROM book WHERE content_hash=:content_hash), :author_identity_id, :position, :name, :sort_key, :role)
ON CONFLICT(book_row_id, position) DO UPDATE SET
    author_identity_id=excluded.author_identity_id,
    name=excluded.name,
    sort_name=excluded.sort_name,
    role=excluded.role
WHERE book_contributor.author_identity_id IS NOT excluded.author_identity_id
   OR book_contributor.name IS NOT excluded.name
   OR book_contributor.sort_name IS NOT excluded.sort_name
   OR book_contributor.role IS NOT excluded.role;

-- name: book_addition_snapshots?
-- param: keys_json: &str
SELECT b.title IS NOT NULL,
       v.changed_at,v.conflict_rank,v.replica_id,v.replica_seq,v.mutation_id,
       COALESCE(b.purged_at IS NOT NULL,0), b.row_id IS NOT NULL
FROM json_each(:keys_json) requested
LEFT JOIN book b ON b.content_hash=json_extract(requested.value,'$[1]')
LEFT JOIN sync_state_version v
  ON v.state_kind=json_extract(requested.value,'$[0]')
 AND v.state_key=json_extract(requested.value,'$[1]')
 AND v.state_subkey=json_extract(requested.value,'$[2]')
ORDER BY CAST(requested.key AS INTEGER);

-- name: book_is_purged_select?
-- param: content_hash: &str
SELECT COALESCE((SELECT purged_at IS NOT NULL FROM book WHERE content_hash=:content_hash),0);

-- name: book_toc_upsert!
-- param: content_hash: &str
-- param: entry_count: i64
-- param: toc_json: &str
INSERT INTO book_toc(content_hash,entry_count,toc_json)
VALUES(:content_hash,:entry_count,:toc_json)
ON CONFLICT(content_hash) DO UPDATE SET
    entry_count=excluded.entry_count,
    toc_json=excluded.toc_json;

-- name: contributors_match_batch?
-- param: keys_json: &str
SELECT CAST(requested.key AS INTEGER), c.position, a.stable_id, c.name, c.sort_name, c.role
FROM json_each(:keys_json) requested
JOIN book b ON b.content_hash=requested.value
JOIN book_contributor c ON c.book_row_id=b.row_id
LEFT JOIN author_identity a ON a.id=c.author_identity_id
ORDER BY CAST(requested.key AS INTEGER), c.position;

-- name: directory_projection_rows_select?
SELECT id,parent_id,name,deleted_at IS NOT NULL FROM dir;

-- name: ensure_remote_books_batch!
-- param: missing_json: &str
INSERT OR IGNORE INTO book(content_hash,added_at)
SELECT json_extract(value,'$[0]'),json_extract(value,'$[1]')
FROM json_each(:missing_json) ORDER BY CAST(key AS INTEGER);

-- name: ensure_remote_directory_placeholder!
-- param: dir_id: &str
INSERT OR IGNORE INTO dir(id,parent_id,name,intent_parent_id,intent_name,intent_lifecycle,deleted_at,purged_at)
VALUES(:dir_id,'00000000-0000-0000-0000-000000000000','Pending','00000000-0000-0000-0000-000000000000','Pending',1,1,NULL);

-- name: first_book_insert!
-- param: content_hash: &str
-- param: added_at: i64
INSERT OR IGNORE INTO book    (content_hash, added_at)
VALUES              (:content_hash, :added_at);

-- name: get_remote_directory_intents?
SELECT id, COALESCE(intent_parent_id, parent_id), COALESCE(intent_name, name),
       COALESCE(intent_lifecycle, CASE WHEN purged_at IS NOT NULL THEN 2 WHEN deleted_at IS NOT NULL THEN 1 ELSE 0 END)
FROM dir
WHERE id != '00000000-0000-0000-0000-000000000000';

-- name: hide_remote_directories!
UPDATE dir SET deleted_at=1, purged_at=NULL WHERE id != '00000000-0000-0000-0000-000000000000';

-- name: mark_remote_directory_purged!
-- param: dir_id: &str
UPDATE dir SET deleted_at=1, purged_at=1 WHERE id=:dir_id;

-- name: meta_before_contributor_fast_path_update!
-- param: content_hash: &str
-- param: title: &str
UPDATE book SET title=:title WHERE content_hash=:content_hash AND title IS NOT :title;

-- name: clear_book_collections!
-- param: book_row_id: i64
DELETE FROM book_collection WHERE book_row_id=:book_row_id;

-- name: clear_book_identifiers!
-- param: book_row_id: i64
DELETE FROM book_identifier WHERE book_row_id=:book_row_id;

-- name: clear_book_languages!
-- param: book_row_id: i64
DELETE FROM book_language WHERE book_row_id=:book_row_id;

-- name: clear_book_dates!
-- param: book_row_id: i64
DELETE FROM book_date WHERE book_row_id=:book_row_id;

-- name: clear_book_publishers!
-- param: book_row_id: i64
DELETE FROM book_publisher WHERE book_row_id=:book_row_id;

-- name: meta_ensure_external_subject_nodes_insert!
-- param: system_id: &str
INSERT OR IGNORE INTO subject_system(id,version,matcher_version) VALUES(:system_id,'external',1);

-- name: meta_ensure_external_subject_nodes_insert_2!
-- param: system_id: &str
-- param: path: &str
-- param: parent_path: &str
-- param: component: &str
INSERT OR IGNORE INTO subject_node(system_id,path,parent_path,name,code,assignable) VALUES(:system_id,:path,:parent_path,:component,NULL,1);

-- name: meta_ensure_external_subject_nodes_update!
-- param: system_id: &str
-- param: matcher_version: i64
UPDATE subject_system SET matcher_version=MAX(matcher_version,:matcher_version) WHERE id=:system_id;

-- name: insert_book_collection!
-- param: book_row_id: i64
-- param: position: i64
-- param: name: &str
-- param: collection_type: Option<&str>
-- param: group_position: Option<&str>
INSERT INTO book_collection(book_row_id,position,name,collection_type,group_position)
VALUES(:book_row_id,:position,:name,:collection_type,:group_position);

-- name: insert_book_identifier!
-- param: book_row_id: i64
-- param: position: i64
-- param: scheme: &str
-- param: value: &str
-- param: canonical_value: Option<&str>
-- param: source: &str
-- param: scope: &str
INSERT INTO book_identifier(book_row_id,position,scheme,value,canonical_value,source,scope)
VALUES(:book_row_id,:position,:scheme,:value,:canonical_value,:source,:scope);

-- name: insert_book_language!
-- param: book_row_id: i64
-- param: position: i64
-- param: language_tag: &str
INSERT INTO book_language(book_row_id,position,language_tag) VALUES(:book_row_id,:position,:language_tag);

-- name: insert_book_date!
-- param: book_row_id: i64
-- param: position: i64
-- param: value: &str
-- param: normalized_year: Option<i32>
-- param: normalized_month: Option<i64>
-- param: normalized_day: Option<i64>
-- param: event: Option<&str>
INSERT INTO book_date(book_row_id,position,value,normalized_year,normalized_month,normalized_day,event)
VALUES(:book_row_id,:position,:value,:normalized_year,:normalized_month,:normalized_day,:event);

-- name: insert_book_value!
-- param: book_row_id: i64
-- param: kind: &str
-- param: position: i64
-- param: value: &str
-- param: qualifier: Option<&str>
-- param: secondary_value: Option<&str>
INSERT INTO book_value(book_row_id,kind,position,value,qualifier,secondary_value)
VALUES(:book_row_id,:kind,:position,:value,:qualifier,:secondary_value);

-- name: insert_book_publisher!
-- param: book_row_id: i64
-- param: publisher_id: i64
-- param: agent_identity_id: i64
-- param: position: i64
-- param: credited_name: &str
INSERT INTO book_publisher(book_row_id,publisher_id,agent_identity_id,position,credited_name)
VALUES(:book_row_id,:publisher_id,:agent_identity_id,:position,:credited_name);

-- name: meta_replace_prepared_projections_delete!
-- param: book_row_id: i64
-- param: kind: &str
DELETE FROM book_value WHERE book_row_id=:book_row_id AND kind=:kind;

-- name: meta_replace_prepared_projections_select?
-- param: content_hash: &str
SELECT row_id FROM book WHERE content_hash=:content_hash;

-- name: meta_stored_book_select?
-- param: content_hash: &str
SELECT book_metadata FROM book WHERE content_hash=:content_hash;

-- name: meta_update_book_metadata_state_preserving_ids_update_2!
-- param: content_hash: &str
-- param: title: &str
-- param: book_metadata: &[u8]
UPDATE book SET title=:title,book_metadata=:book_metadata
        WHERE content_hash=:content_hash AND (title IS NOT :title OR book_metadata IS NOT :book_metadata);

-- name: pdf_reader_metadata_upsert!
-- param: content_hash: &str
-- param: checksum: &str
-- param: metadata: &[u8]
INSERT INTO pdf_reader_metadata VALUES(:content_hash,:checksum,:metadata) ON CONFLICT(content_hash,checksum) DO UPDATE SET metadata=excluded.metadata WHERE metadata IS NOT excluded.metadata;

-- name: project_remote_directory!
-- param: dir_id: &str
-- param: parent_id: &str
-- param: name: &str
UPDATE dir SET parent_id=:parent_id, name=:name WHERE id=:dir_id;

-- name: book_batch_select?
-- param: keys_json: &str
SELECT b.book_metadata
FROM json_each(:keys_json) requested
LEFT JOIN book b ON b.content_hash=requested.value
ORDER BY CAST(requested.key AS INTEGER);

-- name: queue_book_work_batch!
-- param: queue_json: &str
INSERT OR REPLACE INTO local_book_work(content_hash)
SELECT value FROM json_each(:queue_json) ORDER BY CAST(key AS INTEGER);

-- name: queue_book_work_insert!
-- param: content_hash: &str
INSERT OR REPLACE INTO local_book_work(content_hash) VALUES(:content_hash);

-- name: queue_directory_work_insert!
-- param: dir_id: &str
INSERT OR REPLACE INTO local_directory_work(dir_id) VALUES(:dir_id);

-- name: read_sync_state_version_select?
-- param: state_kind: &str
-- param: state_key: &str
-- param: state_subkey: &str
SELECT changed_at,conflict_rank,replica_id,replica_seq,mutation_id FROM sync_state_version
        WHERE state_kind=:state_kind AND state_key=:state_key AND state_subkey=:state_subkey;

-- name: rebuild_folder_book_ownership&
DELETE FROM folder_book_ownership;
INSERT INTO folder_book_ownership(folder_id, book_row_id, placement_count, downloaded_placement_count)
WITH RECURSIVE ancestry(descendant_id, ancestor_id) AS (
    SELECT id, id
    FROM dir
    WHERE deleted_at IS NULL

    UNION ALL

    SELECT ancestry.descendant_id, parent.id
    FROM ancestry
    JOIN dir AS current ON current.id=ancestry.ancestor_id
    JOIN dir AS parent ON parent.id=current.parent_id AND parent.deleted_at IS NULL
    WHERE current.id != current.parent_id
)
SELECT
    ancestry.ancestor_id,
    placement.book_row_id,
    COUNT(*),
    SUM(CASE WHEN placement.is_downloaded=1 THEN 1 ELSE 0 END)
    FROM book_dir AS placement
    JOIN ancestry ON ancestry.descendant_id=placement.dir_id
    WHERE placement.deleted_at IS NULL
    GROUP BY ancestry.ancestor_id, placement.book_row_id;

-- name: record_sync_state_version_insert!
-- param: state_kind: &str
-- param: state_key: &str
-- param: state_subkey: &str
-- param: changed_at: i64
-- param: conflict_rank: i64
-- param: replica_id: &str
-- param: replica_seq: i64
-- param: mutation_id: &str
INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id)
        VALUES(:state_kind,:state_key,:state_subkey,:changed_at,:conflict_rank,:replica_id,:replica_seq,:mutation_id)
        ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,
          conflict_rank=excluded.conflict_rank,replica_id=excluded.replica_id,replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id;

-- name: record_versions_batch!
-- param: versions_json: &str
INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id)
        SELECT json_extract(value,'$[0]'),json_extract(value,'$[1]'),json_extract(value,'$[2]'),
               json_extract(value,'$[3]'),json_extract(value,'$[4]'),json_extract(value,'$[5]'),
               json_extract(value,'$[6]'),json_extract(value,'$[7]')
        FROM json_each(:versions_json) WHERE true ORDER BY CAST(key AS INTEGER)
        ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,
          conflict_rank=excluded.conflict_rank,replica_id=excluded.replica_id,replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id;

-- name: remote_book_has_metadata?
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=:content_hash AND title IS NOT NULL);

-- name: set_remote_directory_lifecycle!
-- param: dir_id: &str
-- param: lifecycle: i32
UPDATE dir SET intent_lifecycle=:lifecycle WHERE id=:dir_id;

-- name: set_remote_directory_name!
-- param: dir_id: &str
-- param: name: &str
UPDATE dir SET intent_name=:name WHERE id=:dir_id;

-- name: set_remote_directory_parent!
-- param: dir_id: &str
-- param: parent_id: &str
UPDATE dir SET intent_parent_id=:parent_id WHERE id=:dir_id;

-- name: upsert_remote_annotation!
-- param: id: &str
-- param: content_hash: &str
-- param: anchor_kind: &str
-- param: anchor_cfi: Option<&str>
-- param: anchor_page: Option<i64>
-- param: anchor_rects: Option<&[u8]>
-- param: anchor_fallback_cfi: Option<&str>
-- param: exact_text: &str
-- param: style: &str
-- param: color: &str
-- param: note: &str
-- param: created_at: i64
-- param: modified_at: i64
-- param: deleted_at: Option<i64>
INSERT INTO annotation(id,book_row_id,anchor_kind,anchor_cfi,anchor_page,anchor_rects,anchor_fallback_cfi,exact_text,style,color,note,created_at,modified_at,deleted_at)
VALUES(:id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:anchor_kind,:anchor_cfi,:anchor_page,:anchor_rects,:anchor_fallback_cfi,:exact_text,:style,:color,:note,:created_at,:modified_at,:deleted_at)
ON CONFLICT(id) DO UPDATE SET
    book_row_id=excluded.book_row_id,anchor_kind=excluded.anchor_kind,anchor_cfi=excluded.anchor_cfi,
    anchor_page=excluded.anchor_page,anchor_rects=excluded.anchor_rects,
    anchor_fallback_cfi=excluded.anchor_fallback_cfi,exact_text=excluded.exact_text,
    style=excluded.style,color=excluded.color,
    note=excluded.note,created_at=excluded.created_at,modified_at=excluded.modified_at,
    deleted_at=excluded.deleted_at;

-- name: schema_books &
-- Book rows, book metadata projection, and subject taxonomy.
create table if not exists book(
    row_id          INTEGER primary key,
    -- Portable identity used by sync and asset APIs. Local relationships use
    -- row_id so their indexes remain compact and integer joins stay cheap.
    content_hash    TEXT not null unique,
    title           TEXT,
    description_scanned INTEGER not null default 0,
    -- Canonical synchronized bibliographic metadata. Dedicated tables below
    -- are disposable query projections rebuilt from this value.
    book_metadata BLOB not null default X'',
    -- Canonical extension of the book's format ('epub', 'pdf', ...). Fixed for
    -- the life of the row: identity is derived from the bytes, so the format
    -- cannot change without becoming a different book. Empty only on a row that
    -- has never been observed on any device's disk.
    format          TEXT not null default '',
    -- Device-local library visibility. This is intentionally not synchronized.
    hidden_at       INTEGER,
    read_pos        TEXT                DEFAULT NULL,
    read_progress   REAL                DEFAULT 0,
    added_at        INTEGER,
    deleted_at      INTEGER,
    purged_at       INTEGER,
    trash_origin_dir_id TEXT,
    CHECK (purged_at IS NULL OR deleted_at IS NOT NULL)
);

create table if not exists publisher(
    id              INTEGER primary key,
    name            TEXT not null,
    normalized_name TEXT not null unique
);
create table if not exists book_publisher(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    publisher_id    INTEGER not null references publisher(id) on delete cascade,
    agent_identity_id INTEGER not null references author_identity(id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    credited_name   TEXT not null,
    primary key(book_row_id, position)
);
create index if not exists idx_book_publisher_publisher on book_publisher(publisher_id, book_row_id);
create index if not exists idx_book_publisher_agent on book_publisher(agent_identity_id, book_row_id);
create table if not exists book_identifier(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    scheme          TEXT not null,
    value           TEXT not null,
    canonical_value TEXT,
    source          TEXT not null,
    scope           TEXT not null default 'book' check(scope in ('book','edition','work')),
    primary key(book_row_id, position)
);
create index if not exists idx_book_identifier_lookup on book_identifier(scheme, value, book_row_id);
create table if not exists book_language(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    language_tag    TEXT not null,
    primary key(book_row_id, position)
);
create index if not exists idx_book_language_tag on book_language(language_tag, book_row_id);
-- Merged relational contributor register. Every role (including "aut" =
-- author) shares one flat per-book position sequence. This replaces the
-- former separate book_author table; author_credit (see views.sql) exposes
-- the author-only subset for call sites that only care about authorship.
create table if not exists book_contributor(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    author_identity_id INTEGER not null references author_identity(id) on delete cascade,
    name            TEXT not null,
    sort_name       TEXT,
    role            BLOB not null check(typeof(role) = 'blob' and length(role) = 3),
    primary key(book_row_id, position)
);
create index if not exists idx_book_contributor_identity on book_contributor(author_identity_id, book_row_id);
create table if not exists book_subject(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    name            TEXT not null,
    source          TEXT not null,
    authority       TEXT,
    code            TEXT,
    primary key(book_row_id, position)
);
create index if not exists idx_book_subject_name on book_subject(name, book_row_id);
create index if not exists idx_book_subject_authority on book_subject(authority, book_row_id);
create table if not exists book_subject_code(
    book_row_id     INTEGER not null,
    subject_position INTEGER not null,
    system_id       TEXT not null,
    code            TEXT not null,
    primary key(book_row_id, subject_position, system_id, code),
    foreign key(book_row_id, subject_position) references book_subject(book_row_id, position) on delete cascade
);
create index if not exists idx_book_subject_code_lookup on book_subject_code(system_id, code, book_row_id);

-- Controlled taxonomies and disposable automatic assignments. The raw
-- declarations above remain the evidence and synchronization boundary.
create table if not exists subject_system(
    id              TEXT primary key,
    version         TEXT not null,
    matcher_version INTEGER not null check(matcher_version >= 0)
);
create table if not exists subject_node(
    system_id       TEXT not null references subject_system(id) on delete cascade,
    path            TEXT not null,
    parent_path     TEXT not null,
    name            TEXT not null,
    code            TEXT,
    assignable      INTEGER not null check(assignable in (0, 1)),
    primary key(system_id, path),
    unique(system_id, code)
);
create index if not exists idx_subject_node_parent on subject_node(system_id, parent_path, name);
create table if not exists book_subject_assignment(
    book_row_id     INTEGER not null references book(row_id) on delete cascade,
    system_id       TEXT not null,
    subject_path    TEXT not null,
    matcher_version INTEGER not null check(matcher_version >= 0),
    primary key(book_row_id, system_id, subject_path),
    foreign key(system_id, subject_path) references subject_node(system_id, path) on delete cascade
);
create index if not exists idx_book_subject_assignment_subject on book_subject_assignment(system_id, subject_path, book_row_id);
create table if not exists book_subject_assignment_evidence(
    book_row_id     INTEGER not null,
    system_id       TEXT not null,
    subject_path    TEXT not null,
    subject_position INTEGER not null,
    primary key(book_row_id, system_id, subject_path, subject_position),
    foreign key(book_row_id, system_id, subject_path) references book_subject_assignment(book_row_id, system_id, subject_path) on delete cascade,
    foreign key(book_row_id, subject_position) references book_subject(book_row_id, position) on delete cascade
);
create table if not exists subject_code_mapping(
    source_system_id TEXT not null,
    source_code      TEXT not null,
    unified_path     TEXT not null,
    mapper_version   INTEGER not null check(mapper_version >= 0),
    primary key(source_system_id, source_code, unified_path)
);
create index if not exists idx_subject_code_mapping_path on subject_code_mapping(unified_path, source_system_id, source_code);

-- Curated subjects and navigation live in the attached read-only taxonomy.
-- Library storage contains book assignments and their evidence only.
create table if not exists source_subject(
    system_id       TEXT not null,
    source_version  TEXT not null,
    code            TEXT not null,
    official_label  TEXT,
    primary key(system_id,source_version,code)
);
create table if not exists source_concept_mapping(
    source_system_id TEXT not null,
    source_version   TEXT not null,
    source_code      TEXT not null,
    concept_id INTEGER not null check(concept_id > 0),
    mapping_type     TEXT not null check(mapping_type in ('exact','broader','narrower','related')),
    mapper_version   INTEGER not null check(mapper_version >= 0),
    primary key(source_system_id,source_version,source_code,concept_id),
    foreign key(source_system_id,source_version,source_code) references source_subject(system_id,source_version,code) on delete cascade
);
create index if not exists idx_source_concept_mapping_concept on source_concept_mapping(concept_id,source_system_id,source_code);
create table if not exists book_unified_concept(
    book_row_id     INTEGER not null references book(row_id) on delete cascade,
    concept_id INTEGER not null check(concept_id > 0),
    mapper_version  INTEGER not null check(mapper_version >= 0),
    primary key(book_row_id,concept_id)
);
create index if not exists idx_book_unified_concept_concept on book_unified_concept(concept_id,book_row_id);
create table if not exists book_unified_concept_evidence(
    book_row_id      INTEGER not null,
    concept_id INTEGER not null,
    subject_position INTEGER not null,
    source_system_id TEXT not null,
    source_code      TEXT not null,
    primary key(book_row_id,concept_id,subject_position,source_system_id,source_code),
    foreign key(book_row_id,concept_id) references book_unified_concept(book_row_id,concept_id) on delete cascade,
    foreign key(book_row_id,subject_position,source_system_id,source_code) references book_subject_code(book_row_id,subject_position,system_id,code) on delete cascade
);

-- Local bookkeeping for explicit external enrichment. Provider vocabularies
-- are never cached here; this records only whether one book was attempted.

-- name: schema_authors &
-- Author identity and external identifiers.
create table if not exists author_identity(
    id              INTEGER primary key autoincrement,
    stable_id       TEXT not null unique,
    preferred_name  TEXT not null,
    -- Name matching only selects an unresolved candidate. stable_id and
    -- authority identifiers remain the identity boundary for real people.
    normalized_name TEXT not null,
    is_provisional  INTEGER not null default 1 check(is_provisional in (0, 1)),
    created_at      INTEGER not null
);

create unique index if not exists idx_author_identity_stable_id
on author_identity(stable_id);

create index if not exists idx_author_identity_normalized_name
on author_identity(normalized_name, is_provisional);

create table if not exists author_external_identifier(
    author_identity_id INTEGER not null references author_identity(id) on delete cascade,
    authority          TEXT not null check(authority in ('wikidata', 'open_library', 'viaf', 'isni', 'orcid', 'library_of_congress')),
    external_id        TEXT not null,
    verified_at        INTEGER not null,
    primary key(authority, external_id)
);

-- name: schema_descriptions &
-- Canonical description storage, kept off the frequently read book row.
CREATE TABLE IF NOT EXISTS book_description(
    book_row_id INTEGER PRIMARY KEY REFERENCES book(row_id) ON DELETE CASCADE,
    description TEXT NOT NULL
);

-- name: schema_description_triggers &
-- Description change triggers.
CREATE TRIGGER book_description_insert AFTER INSERT ON book_description

BEGIN
    UPDATE subject_browse_state SET revision=revision+1 WHERE id=1;
    INSERT INTO sync_outbox(state_kind,state_key,body,changed_at,conflict_rank)
    SELECT 'description',b.content_hash,bokheim_mutation('description',b.content_hash,NEW.description),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    FROM book b WHERE b.row_id=NEW.book_row_id
      AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER book_description_update AFTER UPDATE OF description ON book_description
WHEN OLD.description IS NOT NEW.description
BEGIN
    UPDATE subject_browse_state SET revision=revision+1 WHERE id=1;
    INSERT INTO sync_outbox(state_kind,state_key,body,changed_at,conflict_rank)
    SELECT 'description',b.content_hash,bokheim_mutation('description',b.content_hash,NEW.description),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    FROM book b WHERE b.row_id=NEW.book_row_id
      AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;
CREATE TRIGGER book_description_delete AFTER DELETE ON book_description

BEGIN
    UPDATE subject_browse_state SET revision=revision+1 WHERE id=1;
    INSERT INTO sync_outbox(state_kind,state_key,body,changed_at,conflict_rank)
    SELECT 'description',b.content_hash,bokheim_mutation('description',b.content_hash,''),
      CAST((julianday('now')-2440587.5)*86400000 AS INTEGER),0
    FROM book b WHERE b.row_id=OLD.book_row_id
      AND COALESCE((SELECT change_origin FROM sync_metadata WHERE singleton = 1),'local')='local'
    ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET
      replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id,
      body=excluded.body,changed_at=excluded.changed_at,conflict_rank=excluded.conflict_rank;
END;

-- name: schema_contents &
-- Book navigation, reading entry, and audiobook metadata.
-- The book's own navigation document, whatever the format called it:
-- a nav document, an outline, or a chapter list. Stored so the library can draw
-- a book's contents without opening the file — which may be remote, and which
-- the reader would otherwise re-parse on every visit. One row per book, holding
-- the whole tree: it is always read whole and nothing joins against its
-- entries, which is why it is JSON rather than a row per entry. An existing row
-- with no entries records that the file was inspected and carries no
-- navigation, so it is not parsed again.
--
-- Keyed by the book's identity, not by the bytes it was read from: an identity
-- survives a revision, and so do the targets, because a PDF's pagination and an
-- audiobook's offsets are both fixed by what the identity is derived from.
create table if not exists book_toc(
    content_hash    TEXT primary key not null,
    entry_count     INTEGER not null check(entry_count >= 0),
    toc_json        TEXT not null
);

-- Which navigation entry the reader was inside when it last saved a position.
-- Derived from the position and the book's own navigation, so it is local and
-- never synchronized: another device recomputes it when it opens the book.
-- Stored as the entry's target rather than its title, so a re-parsed navigation
-- document supplies the wording.
create table if not exists book_reading_entry(
    content_hash    TEXT primary key not null,
    target          TEXT not null
);

-- What an audiobook is, beyond its navigation. The chapters themselves live in
-- book_toc with every other format's; the duration stays here because it is not
-- navigation, and because it is what turns a chapter's start into its end.
create table if not exists audiobook_metadata(
    content_hash    TEXT primary key not null,
    duration_ms     INTEGER not null check(duration_ms >= 0)
);

-- name: reserve_write_lock!
UPDATE main.book SET row_id=row_id WHERE 0;

-- name: outbox_delete_for_ids!
-- param: payload: &str
DELETE FROM sync_outbox WHERE mutation_id IN (SELECT value FROM json_each(:payload));

-- name: change_origin_select?
SELECT change_origin FROM sync_metadata WHERE singleton = 1;

-- name: mark_origin_remote!
INSERT INTO sync_metadata(singleton, change_origin) VALUES (1, 'remote') ON CONFLICT(singleton) DO UPDATE SET change_origin = excluded.change_origin;

-- name: restore_origin!
-- param: origin: &str
INSERT INTO sync_metadata(singleton, change_origin) VALUES (1, :origin) ON CONFLICT(singleton) DO UPDATE SET change_origin = excluded.change_origin;

-- name: read_position_update!
-- param: location: &str
-- param: progress: f32
-- param: content_hash: &str
UPDATE book SET read_pos = :location, read_progress = COALESCE(:progress, read_progress) WHERE content_hash = :content_hash;
