-- name: create_author_table!
create table if not exists author(
    id              INTEGER primary key autoincrement,
    -- A normalized name cluster, not proof that every matching credit is the
    -- same real person. Resolved identities are represented separately below.
    name            TEXT not null unique,
    normalized_name TEXT not null
);

-- name: create_author_normalized_name_index!
create unique index if not exists idx_author_normalized_name
on author(normalized_name);

-- name: create_author_identity_table!
create table if not exists author_identity(
    id              INTEGER primary key autoincrement,
    stable_id       TEXT not null unique,
    preferred_name  TEXT not null,
    is_provisional  INTEGER not null default 1 check(is_provisional in (0, 1)),
    created_at      INTEGER not null
);

-- name: create_author_identity_stable_id_index!
create unique index if not exists idx_author_identity_stable_id
on author_identity(stable_id);

-- name: create_author_external_identifier_table!
create table if not exists author_external_identifier(
    author_identity_id INTEGER not null references author_identity(id) on delete cascade,
    authority          TEXT not null check(authority in ('wikidata', 'open_library', 'viaf', 'isni', 'orcid', 'library_of_congress')),
    external_id        TEXT not null,
    verified_at        INTEGER not null,
    primary key(authority, external_id)
);

-- One normalized spelling may belong to multiple people, and one person may
-- have multiple spellings. This table records that many-to-many possibility.
-- name: create_author_identity_name_table!
create table if not exists author_identity_name(
    author_identity_id INTEGER not null references author_identity(id) on delete cascade,
    author_id          INTEGER not null references author(id) on delete cascade,
    primary key(author_identity_id, author_id)
);

-- name: create_book_table!
create table if not exists book(
    id              TEXT primary key,
    title           TEXT,
    author_id       INTEGER references author(id) on delete set null,
    -- The exact synchronized spelling for this book. author_id only groups
    -- equivalent spellings for the library's author index.
    author_name     TEXT not null default '',
    -- Canonical ordered author credits encoded with the shared binary wire
    -- codec. book_author is the relational query projection of this register.
    authors         BLOB not null default X'424B484D0500',
    description     TEXT not null default '',
    description_scanned INTEGER not null default 0,
    -- Canonical synchronized bibliographic metadata. Dedicated tables below
    -- are disposable query projections rebuilt from this value.
    publication_metadata BLOB not null default X'',
    -- Device-local library visibility. This is intentionally not synchronized.
    hidden_at       INTEGER,
    read_pos        TEXT                DEFAULT NULL,
    read_progress   REAL                DEFAULT 0,
    last_read       INTEGER             DEFAULT 0,
    added_at        INTEGER,
    -- Total version of the last add/delete that won this book's lifecycle
    -- state. The deletion rank is derived from deleted_at.
    version_changed_at          INTEGER not null default 0,
    version_tie_breaker           INTEGER not null default 0,
    version_replica_id       TEXT not null default '',
    version_replica_seq         INTEGER not null default 0,
    version_mutation_id       TEXT not null default '',
    deleted_at      INTEGER
);

-- name: create_book_author_table!
create table if not exists book_author(
    book_id         TEXT not null references book(id) on delete cascade,
    author_id       INTEGER not null references author(id) on delete cascade,
    author_identity_id INTEGER references author_identity(id) on delete set null,
    position        INTEGER not null check(position >= 0),
    credited_name   TEXT not null,
    primary key(book_id, position)
);

-- name: create_book_author_author_index!
create index if not exists idx_book_author_author
on book_author(author_id, book_id);

-- name: create_book_author_identity_index!
create index if not exists idx_book_author_identity
on book_author(author_identity_id, book_id) where author_identity_id is not null;

-- name: create_publication_metadata_projection_tables &
create table if not exists publisher(
    id              INTEGER primary key,
    name            TEXT not null,
    normalized_name TEXT not null unique
);
create table if not exists book_publisher(
    book_id         TEXT not null references book(id) on delete cascade,
    publisher_id    INTEGER not null references publisher(id) on delete cascade,
    agent_identity_id INTEGER not null references author_identity(id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    credited_name   TEXT not null,
    primary key(book_id, position)
);
create index if not exists idx_book_publisher_publisher on book_publisher(publisher_id, book_id);
create index if not exists idx_book_publisher_agent on book_publisher(agent_identity_id, book_id);
create table if not exists book_identifier(
    book_id         TEXT not null references book(id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    scheme          TEXT not null,
    value           TEXT not null,
    canonical_value TEXT,
    type_evidence   BLOB not null,
    primary key(book_id, position)
);
create index if not exists idx_book_identifier_lookup on book_identifier(scheme, value, book_id);
create table if not exists book_language(
    book_id         TEXT not null references book(id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    language_tag    TEXT not null,
    primary key(book_id, position)
);
create index if not exists idx_book_language_tag on book_language(language_tag, book_id);
create table if not exists book_contributor(
    book_id         TEXT not null references book(id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    agent_identity_id INTEGER not null references author_identity(id) on delete cascade,
    name            TEXT not null,
    sort_name       TEXT,
    role            BLOB not null check(typeof(role) = 'blob' and length(role) = 3),
    primary key(book_id, position)
);
create index if not exists idx_book_contributor_agent on book_contributor(agent_identity_id, book_id);
create table if not exists book_subject(
    book_id         TEXT not null references book(id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    name            TEXT not null,
    authority       TEXT,
    code            TEXT,
    primary key(book_id, position)
);
create index if not exists idx_book_subject_name on book_subject(name, book_id);
create table if not exists book_collection(
    book_id         TEXT not null references book(id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    name            TEXT not null,
    collection_type TEXT,
    group_position  TEXT,
    primary key(book_id, position)
);
create table if not exists book_publication_date(
    book_id         TEXT not null references book(id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    value           TEXT not null,
    normalized_year INTEGER,
    normalized_month INTEGER,
    normalized_day INTEGER,
    event           TEXT,
    primary key(book_id, position)
);
create table if not exists book_publication_value(
    book_id         TEXT not null references book(id) on delete cascade,
    kind            TEXT not null,
    position        INTEGER not null check(position >= 0),
    value           TEXT not null,
    qualifier       TEXT,
    secondary_value TEXT,
    primary key(book_id, kind, position)
);
create index if not exists idx_book_publication_value_lookup on book_publication_value(kind, value, book_id);

-- name: create_catalog_import_table!
create table if not exists catalog_import(
    source_id        TEXT not null,
    publication_id   TEXT not null,
    work_id          TEXT,
    acquisition_url  TEXT not null,
    book_id          TEXT not null references book(id) on delete cascade,
    imported_at      INTEGER not null,
    primary key (source_id, publication_id)
);

-- name: create_catalog_import_work_index!
create index if not exists idx_catalog_import_work
on catalog_import(source_id, work_id) where work_id is not null;

-- name: create_dir_table!
create table if not exists dir(
    id              TEXT primary key,
    parent_id       TEXT not null references dir(id) on delete cascade,
    name            TEXT not null,
    -- Authoritative synchronized register. The parent/name/deleted_at columns
    -- above are the derived, filesystem-safe projection and may temporarily
    -- suppress an otherwise-present intent to break cycles/path collisions.
    intent_parent_id TEXT,
    intent_name      TEXT,
    intent_deleted   INTEGER,
    version_changed_at          INTEGER not null default 0,
    version_tie_breaker           INTEGER not null default 0,
    version_replica_id       TEXT not null default '',
    version_replica_seq         INTEGER not null default 0,
    version_mutation_id       TEXT not null default '',
    deleted_at      INTEGER,
    CHECK (id != '__root__' OR deleted_at IS NULL)
);

-- name: create_live_dir_path_index!
CREATE UNIQUE INDEX IF NOT EXISTS idx_dir_live_path
ON dir(parent_id, name) WHERE deleted_at IS NULL AND id != '__root__';

-- name: insert_root!
insert into dir(id, parent_id, name, intent_parent_id, intent_name, intent_deleted, deleted_at)
values ('__root__', '__root__', '', '__root__', '', 0, NULL)
on conflict(id) do update set parent_id = excluded.parent_id, name = excluded.name, deleted_at = NULL;

-- name: create_book_dir_table!
create table if not exists book_dir(
    dir_id          TEXT not null references dir(id) on delete cascade,
    book_id         TEXT not null references book(id) on delete cascade,
    file_name       TEXT not null,
    -- Per-machine filesystem fingerprint (mtime/size/inode/dev). Cheap to
    -- compute on every scan so unchanged files can be skipped without re-reading
    -- their contents (see get_book_dir_by_hash). Not unique and not portable
    -- across devices, so it is an attribute, not the key.
    local_hash      TEXT not null,
    -- Portable content-addressed blob key: full-file blake3 hex. This is what
    -- the sync server stores/verifies and what `book_dir_added` events carry.
    -- Many rows may share it (the same book placed in several folders).
    content_hash    TEXT not null default '',
    is_downloaded   INTEGER not null default 0,
    -- Monotonic scan generation in which this placement was last seen on disk.
    -- After a full scan, locally-present rows (is_downloaded = 1) not stamped
    -- with the current generation are reconciled as vanished. Remote placements
    -- never downloaded keep 0 and are excluded via the is_downloaded filter.
    last_scan       INTEGER not null default 0,
    -- Total version of the last add/remove that won this placement, making it
    -- an LWW register. Local mutations stamp it from the canonical outbox; remote
    -- applies gate on it (see apply_remote_book_dir) so concurrent operations
    -- converge even when their wall-clock version components are equal.
    version_changed_at          INTEGER not null default 0,
    version_tie_breaker           INTEGER not null default 0,
    version_replica_id       TEXT not null default '',
    version_replica_seq         INTEGER not null default 0,
    version_mutation_id       TEXT not null default '',
    -- Authoritative placement lifecycle. deleted_at is the derived physical
    -- path projection and may suppress a losing same-path occupant.
    intent_deleted  INTEGER,
    deleted_at      INTEGER,
    -- A row is a file *placement*: one file at one path. Its identity is the
    -- path plus the book it holds, matching mark_book_dir_deleted. Keying on a
    -- hash would collapse the same content placed in multiple locations.
    PRIMARY KEY     (dir_id, book_id, file_name)
);

-- name: create_book_dir_local_hash_index!
-- local_hash lost its implicit PK index when it stopped being the key; the
-- scan-skip lookup queries by it on every file, so index it explicitly.
CREATE INDEX IF NOT EXISTS idx_book_dir_local_hash ON book_dir(local_hash);

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
        'directory', 'book_lifecycle', 'placement', 'reading_position',
        'annotation', 'bookmark', 'book_link', 'last_opened', 'metadata'
    )),
    state_key       TEXT not null,
    body            BLOB not null,
    changed_at      INTEGER not null check(changed_at >= 0),
    conflict_rank   INTEGER not null default 0 check(conflict_rank in (0, 1)),
    placement_dir_id TEXT,
    placement_file_name TEXT,
    placement_book_id TEXT,
    check(
        (state_kind = 'placement' AND placement_dir_id IS NOT NULL AND placement_file_name IS NOT NULL AND placement_book_id IS NOT NULL)
        OR
        (state_kind != 'placement' AND placement_dir_id IS NULL AND placement_file_name IS NULL AND placement_book_id IS NULL)
    ),
    unique(state_kind, state_key)
);

-- name: create_annotation_table!
create table if not exists annotation(
    id              TEXT primary key,
    book_id         TEXT not null references book(id) on delete cascade,
    anchor_kind     TEXT not null check(anchor_kind in ('epub_cfi', 'pdf')),
    anchor_cfi      TEXT,
    anchor_page     INTEGER,
    anchor_rects    BLOB,
    anchor_fallback_cfi TEXT,
    exact_text      TEXT not null,
    prefix          TEXT,
    suffix          TEXT,
    style           TEXT not null check(style in ('highlight', 'underline', 'squiggly', 'strikethrough')),
    color           TEXT not null,
    note            TEXT not null default '',
    created_at      INTEGER not null,
    modified_at     INTEGER not null,
    deleted_at      INTEGER,
    version_changed_at          INTEGER not null default 0,
    version_tie_breaker           INTEGER not null default 0,
    version_replica_id       TEXT not null default '',
    version_replica_seq         INTEGER not null default 0,
    version_mutation_id       TEXT not null default ''
    ,check(
        (anchor_kind='epub_cfi' AND anchor_cfi IS NOT NULL AND anchor_page IS NULL AND anchor_rects IS NULL AND anchor_fallback_cfi IS NULL)
        OR
        (anchor_kind='pdf' AND anchor_cfi IS NULL AND anchor_page IS NOT NULL AND anchor_page >= 0 AND anchor_rects IS NOT NULL)
    )
);

-- name: create_annotation_book_index!
create index if not exists idx_annotation_book on annotation(book_id, deleted_at, modified_at);

-- name: create_bookmark_table!
create table if not exists bookmark(
    id              TEXT primary key,
    book_id         TEXT not null references book(id) on delete cascade,
    cfi             TEXT not null,
    label           TEXT not null default '',
    created_at      INTEGER not null,
    modified_at     INTEGER not null,
    deleted_at      INTEGER,
    version_changed_at          INTEGER not null default 0,
    version_tie_breaker           INTEGER not null default 0,
    version_replica_id       TEXT not null default '',
    version_replica_seq         INTEGER not null default 0,
    version_mutation_id       TEXT not null default ''
);

-- name: create_bookmark_book_index!
create index if not exists idx_bookmark_book on bookmark(book_id, deleted_at, modified_at);

-- User-authored semantic graph edges. Rows are retained after deletion so the
-- tombstone and its LWW version can synchronize to every replica.
-- name: create_book_link_table!
create table if not exists book_link(
    id              TEXT primary key,
    source_book_id  TEXT not null references book(id) on delete cascade,
    target_book_id  TEXT not null references book(id) on delete cascade,
    relation        TEXT not null,
    note            TEXT not null default '',
    created_at      INTEGER not null,
    modified_at     INTEGER not null,
    deleted_at      INTEGER,
    version_changed_at          INTEGER not null default 0,
    version_tie_breaker           INTEGER not null default 0,
    version_replica_id       TEXT not null default '',
    version_replica_seq         INTEGER not null default 0,
    version_mutation_id       TEXT not null default '',
    check(source_book_id != target_book_id)
);

-- name: create_book_link_source_index!
create index if not exists idx_book_link_source on book_link(source_book_id, deleted_at, modified_at);

-- name: create_book_link_target_index!
create index if not exists idx_book_link_target on book_link(target_book_id, deleted_at, modified_at);

-- name: create_sync_metadata_table!
create table if not exists sync_metadata(
    key     TEXT primary key,
    value   TEXT not null
);

-- Remote presence is independent of local filesystem locations. Keeping these
-- as separate facts avoids impossible either-local-or-remote states.
-- name: create_asset_remote_state_table!
create table if not exists asset_remote_state(
    kind            TEXT not null check(kind in ('book', 'thumbnail')),
    hash            TEXT not null check(length(hash) = 64 and hash not glob '*[^0-9a-f]*'),
    presence        INTEGER not null default 0 check(presence in (0, 1, 2)),
    last_checked    INTEGER,
    last_error      TEXT,
    primary key (kind, hash)
);

-- name: create_blob_local_ref_table!
create table if not exists blob_local_ref(
    kind            TEXT not null check(kind in ('book', 'thumbnail')),
    hash            TEXT not null check(length(hash) = 64 and hash not glob '*[^0-9a-f]*'),
    path            TEXT not null,
    exists_flag     INTEGER not null default 0 check(exists_flag in (0, 1)),
    last_seen       INTEGER not null default 0,
    primary key (kind, hash, path)
);

-- Durable exact-content download intent. Rows remain until verified local
-- content exists, regardless of process restarts or transient server absence.
-- name: create_asset_requests_table!
create table if not exists asset_requests(
    kind            TEXT not null check(kind in ('book', 'thumbnail')),
    hash            TEXT not null check(length(hash) = 64 and hash not glob '*[^0-9a-f]*'),
    origin          TEXT not null default 'background' check(origin in ('background', 'user_initiated')),
    created_at      INTEGER not null,
    primary key (kind, hash)
);

-- A request may arrive before synchronized placement metadata reveals a hash.
-- Preserve that logical book request until it can be expanded atomically into
-- one or more asset_requests rows.
-- name: create_book_download_requests_table!
create table if not exists book_download_requests(
    book_id         TEXT primary key,
    origin          TEXT not null default 'user_initiated' check(origin in ('background', 'user_initiated')),
    created_at      INTEGER not null
);

-- name: create_transfer_jobs_table!
create table if not exists transfer_jobs(
    id              INTEGER primary key autoincrement,
    kind            TEXT not null check(kind in ('upload_blob', 'upload_thumbnail', 'download_blob', 'download_thumbnail')),
    key1            TEXT not null check(length(key1) = 64 and key1 not glob '*[^0-9a-f]*'),
    rel_path        TEXT not null default '',
    local_path      TEXT not null default '',
    book_id         TEXT not null default '',
    origin          TEXT not null default 'background' check(origin in ('background', 'user_initiated')),
    attempts        INTEGER not null default 0,
    claimed_at      INTEGER,
    claim_token     TEXT not null default '',
    next_retry_at   INTEGER not null default 0,
    last_error      TEXT,
    created_at      INTEGER not null,
    updated_at      INTEGER not null,
    check(
        (kind = 'upload_blob' and local_path <> '' and ((book_id = '' and rel_path = '') or (book_id <> '' and rel_path <> '')))
        or (kind = 'upload_thumbnail' and rel_path = '' and local_path <> '')
        or (kind = 'download_blob' and book_id <> '' and rel_path <> '' and local_path <> '')
        or (kind = 'download_thumbnail' and book_id <> '' and rel_path = '' and local_path = '')
    ),
    unique(kind, key1, rel_path, local_path, book_id)
);

-- name: create_pending_fs_operations_table!
-- Durable outbox bridging committed SQLite removals to non-transactional
-- filesystem work. Rows disappear only after completion or supersession.
create table if not exists pending_fs_operations(
    id              INTEGER primary key autoincrement,
    kind            TEXT not null check(kind in ('book_file_remove', 'dir_remove')),
    dir_id          TEXT not null default '',
    book_id         TEXT not null default '',
    file_name       TEXT not null default '',
    rel_path        TEXT not null default '',
    attempts        INTEGER not null default 0,
    last_error      TEXT,
    created_at      INTEGER not null,
    unique(kind, dir_id, book_id, file_name, rel_path)
);
