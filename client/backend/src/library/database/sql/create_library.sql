-- name: create_author_identity_table!
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

-- name: create_author_identity_stable_id_index!
create unique index if not exists idx_author_identity_stable_id
on author_identity(stable_id);

-- name: create_author_identity_normalized_name_index!
create index if not exists idx_author_identity_normalized_name
on author_identity(normalized_name, is_provisional);

-- name: create_author_external_identifier_table!
create table if not exists author_external_identifier(
    author_identity_id INTEGER not null references author_identity(id) on delete cascade,
    authority          TEXT not null check(authority in ('wikidata', 'open_library', 'viaf', 'isni', 'orcid', 'library_of_congress')),
    external_id        TEXT not null,
    verified_at        INTEGER not null,
    primary key(authority, external_id)
);

-- name: create_book_table!
create table if not exists book(
    row_id          INTEGER primary key,
    -- Portable identity used by sync and asset APIs. Local relationships use
    -- row_id so their indexes remain compact and integer joins stay cheap.
    content_hash    TEXT not null unique,
    title           TEXT,
    description     TEXT not null default '',
    description_scanned INTEGER not null default 0,
    -- Canonical synchronized bibliographic metadata. Dedicated tables below
    -- are disposable query projections rebuilt from this value.
    publication_metadata BLOB not null default X'',
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

-- name: create_publication_metadata_projection_tables &
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
    type_evidence   BLOB not null,
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

-- Controlled taxonomies and disposable automatic assignments. The raw
-- declarations above remain the evidence and synchronization boundary.
-- name: create_subject_taxonomy_tables &
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

-- Local bookkeeping for explicit external enrichment. Provider vocabularies
-- are never cached here; this records only whether one book was attempted.
-- name: create_external_metadata_attempt_table &
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
create table if not exists book_collection(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    name            TEXT not null,
    collection_type TEXT,
    group_position  TEXT,
    primary key(book_row_id, position)
);
create table if not exists book_publication_date(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    value           TEXT not null,
    normalized_year INTEGER,
    normalized_month INTEGER,
    normalized_day INTEGER,
    event           TEXT,
    primary key(book_row_id, position)
);
create table if not exists book_publication_value(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    kind            TEXT not null,
    position        INTEGER not null check(position >= 0),
    value           TEXT not null,
    qualifier       TEXT,
    secondary_value TEXT,
    primary key(book_row_id, kind, position)
);
create index if not exists idx_book_publication_value_lookup on book_publication_value(kind, value, book_row_id);

-- A library-owned relation from stable catalogue publication identities to
-- content-addressed books. Catalogue configuration, URLs, and cached metadata
-- remain application-wide and are deliberately absent here.
-- name: create_catalog_publication_book_table!
create table if not exists catalog_publication_book(
    source_id       TEXT not null,
    edition_id      TEXT not null,
    work_id         TEXT not null default '',
    book_row_id     INTEGER not null references book(row_id) on delete cascade,
    primary key(source_id, edition_id, work_id)
);

-- Remove the old work-level ownership index. Exact identity lookups use the
-- table's primary key.
-- name: drop_catalog_publication_work_index!
drop index if exists idx_catalog_publication_work;

-- name: create_dir_table!
create table if not exists dir(
    id              TEXT primary key,
    parent_id       TEXT not null references dir(id) on delete cascade,
    name            TEXT not null,
    name_key        TEXT generated always as (portable_name_key(name)) stored,
    -- Authoritative synchronized register. The parent/name/deleted_at columns
    -- above are the derived, filesystem-safe projection and may temporarily
    -- suppress an otherwise-present intent to break cycles/path collisions.
    intent_parent_id TEXT,
    intent_name      TEXT,
    -- 0 = present, 1 = Trash, 2 = permanently purged tombstone.
    intent_lifecycle INTEGER,
    deleted_at      INTEGER,
    purged_at       INTEGER,
    CHECK (purged_at IS NULL OR deleted_at IS NOT NULL),
    CHECK (id != '00000000-0000-0000-0000-000000000000' OR (deleted_at IS NULL AND purged_at IS NULL))
);

-- name: create_live_dir_path_index!
CREATE UNIQUE INDEX IF NOT EXISTS idx_dir_live_path
ON dir(parent_id, name_key) WHERE deleted_at IS NULL AND id != '00000000-0000-0000-0000-000000000000';

-- name: insert_root!
insert into dir(id, parent_id, name, intent_parent_id, intent_name, intent_lifecycle, deleted_at, purged_at)
values ('00000000-0000-0000-0000-000000000000', '00000000-0000-0000-0000-000000000000', '', '00000000-0000-0000-0000-000000000000', '', 0, NULL, NULL)
on conflict(id) do update set parent_id = excluded.parent_id, name = excluded.name, deleted_at = NULL, purged_at = NULL;

-- name: create_book_dir_table!
create table if not exists book_dir(
    dir_id          TEXT not null references dir(id) on delete cascade,
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    file_name       TEXT not null,
    -- Per-machine filesystem fingerprint (nanosecond mtime/size/inode/dev). Cheap to
    -- compute on every scan so unchanged files can be skipped without re-reading
    -- their contents (see get_book_dir_by_hash). Not unique and not portable
    -- across devices, so it is an attribute, not the key.
    local_hash      TEXT not null,
    is_downloaded   INTEGER not null default 0,
    -- Monotonic scan generation in which this placement was last seen on disk.
    -- After a full scan, locally-present rows (is_downloaded = 1) not stamped
    -- with the current generation are reconciled as vanished. Remote placements
    -- never downloaded keep 0 and are excluded via the is_downloaded filter.
    last_scan       INTEGER not null default 0,
    deleted_at      INTEGER,
    trash_origin_dir_id TEXT,
    -- A row is a file *placement*: one book in one directory.
    -- Same-path collisions remain local filesystem invariants enforced by
    -- idx_book_dir_live_path.
    PRIMARY KEY     (dir_id, book_row_id)
);

-- name: create_book_dir_local_hash_index!
-- local_hash lost its implicit PK index when it stopped being the key; the
-- scan-skip lookup queries by it on every file, so index it explicitly.
CREATE INDEX IF NOT EXISTS idx_book_dir_local_hash ON book_dir(local_hash);

-- name: create_book_dir_book_index!
-- The composite primary key starts with dir_id and cannot efficiently answer
-- the common "all placements for this book" lookup by itself.
CREATE INDEX IF NOT EXISTS idx_book_dir_book
ON book_dir(book_row_id, deleted_at, is_downloaded);

-- name: create_live_book_path_index!
CREATE UNIQUE INDEX IF NOT EXISTS idx_book_dir_live_path
ON book_dir(dir_id, file_name) WHERE deleted_at IS NULL;

-- The durable synchronization outbox stores a validated binary protocol body.
-- Placement coordinates are columns because local version stamping must never
-- parse a serialized payload. Registers coalesce by their canonical state key;
-- conditional placement transitions retain nonce identities and ordering.
-- name: create_sync_outbox_table!
create table if not exists sync_outbox(
    replica_seq     INTEGER PRIMARY KEY AUTOINCREMENT,
    mutation_id     TEXT not null default (
        lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-' ||
        lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' ||
        lower(hex(randomblob(6)))
    ) unique,
    state_kind      TEXT not null check(state_kind in (
        'directory_name', 'directory_parent', 'directory_lifecycle',
        'book_lifecycle', 'placement', 'reading_position',
        'annotation', 'bookmark', 'book_link', 'catalog_publication', 'metadata'
    )),
    state_key       TEXT not null,
    state_subkey    TEXT not null default '',
    body            BLOB not null,
    changed_at      INTEGER not null check(changed_at >= 0),
    conflict_rank   INTEGER not null default 0 check(conflict_rank in (0, 1, 2)),
    check(state_subkey is not null),
    unique(state_kind, state_key, state_subkey)
);

-- One durable winning version per independently merged state cell. Domain
-- tables contain materialized values only; all merge ordering lives here.
-- name: create_sync_state_version_table!
create table if not exists sync_state_version(
    state_kind      TEXT not null,
    state_key       TEXT not null,
    state_subkey    TEXT not null default '',
    changed_at      INTEGER not null,
    conflict_rank   INTEGER not null check(conflict_rank in (0, 1, 2)),
    replica_id      TEXT not null,
    replica_seq     INTEGER not null,
    mutation_id     TEXT not null,
    primary key(state_kind, state_key, state_subkey)
);

-- name: create_annotation_table!
create table if not exists annotation(
    id              TEXT primary key,
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    anchor_kind     TEXT not null check(anchor_kind in ('epub_cfi', 'pdf')),
    anchor_cfi      TEXT,
    anchor_page     INTEGER,
    anchor_rects    BLOB,
    anchor_fallback_cfi TEXT,
    exact_text      TEXT not null,
    style           TEXT not null check(style in ('highlight', 'underline', 'squiggly', 'strikethrough')),
    color           TEXT not null,
    note            TEXT not null default '',
    created_at      INTEGER not null,
    modified_at     INTEGER not null,
    deleted_at      INTEGER,
    check(
        (anchor_kind='epub_cfi' AND anchor_cfi IS NOT NULL AND anchor_page IS NULL AND anchor_rects IS NULL AND anchor_fallback_cfi IS NULL)
        OR
        (anchor_kind='pdf' AND anchor_cfi IS NULL AND anchor_page IS NOT NULL AND anchor_page >= 0 AND anchor_rects IS NOT NULL)
    )
);

-- name: create_annotation_book_index!
create index if not exists idx_annotation_book on annotation(book_row_id, deleted_at, modified_at);

-- name: create_bookmark_table!
create table if not exists bookmark(
    id              TEXT primary key,
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    cfi             TEXT not null,
    label           TEXT not null default '',
    created_at      INTEGER not null,
    modified_at     INTEGER not null,
    deleted_at      INTEGER
);

-- name: create_bookmark_book_index!
create index if not exists idx_bookmark_book on bookmark(book_row_id, deleted_at, created_at);

-- User-authored semantic graph edges. Rows are retained after deletion so the
-- tombstone and its LWW version can synchronize to every replica.
-- name: create_book_link_table!
create table if not exists book_link(
    id              TEXT primary key,
    source_book_row_id         INTEGER not null references book(row_id) on delete cascade,
    target_book_row_id         INTEGER not null references book(row_id) on delete cascade,
    relation        TEXT not null,
    note            TEXT not null default '',
    created_at      INTEGER not null,
    modified_at     INTEGER not null,
    deleted_at      INTEGER,
    check(source_book_row_id != target_book_row_id)
);

-- name: create_book_link_source_index!
create index if not exists idx_book_link_source on book_link(source_book_row_id, deleted_at, modified_at);

-- name: create_book_link_target_index!
create index if not exists idx_book_link_target on book_link(target_book_row_id, deleted_at, modified_at);

-- name: create_sync_metadata_table!
create table if not exists sync_metadata(
    key     TEXT primary key,
    value   TEXT not null
);

-- Assets confirmed to exist remotely. Absence means unknown, not missing.
-- name: create_remote_asset_table!
create table if not exists remote_asset(
    kind    TEXT not null check(kind in ('book', 'thumbnail')),
    hash    TEXT not null,
    primary key (kind, hash)
);

-- name: create_blob_local_ref_table!
create table if not exists blob_local_ref(
    kind            TEXT not null check(kind in ('book', 'thumbnail')),
    hash            TEXT not null,
    path            TEXT not null,
    exists_flag     INTEGER not null default 0 check(exists_flag in (0, 1)),
    last_seen       INTEGER not null default 0,
    primary key (kind, hash, path)
);

-- Durable download intent for content-addressed assets.
-- name: create_download_requests_table!
create table if not exists download_requests(
    target_kind     TEXT not null check(target_kind in ('book', 'thumbnail')),
    target_key      TEXT not null,
    origin          TEXT not null default 'background' check(origin in ('background', 'user_initiated')),
    created_at      INTEGER not null,
    primary key (target_kind, target_key)
);

-- name: create_audiobook_metadata_table!
create table if not exists audiobook_metadata(
    content_hash    TEXT primary key not null,
    duration_ms     INTEGER not null check(duration_ms >= 0),
    chapter_count   INTEGER not null check(chapter_count >= 0),
    chapter_source  TEXT not null check(chapter_source in ('embedded_m4b', 'audnexus', 'full_audiobook_fallback')),
    chapters_json   TEXT not null
);
