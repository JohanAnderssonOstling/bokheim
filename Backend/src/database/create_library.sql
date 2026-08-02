-- name: create_author_table!
create table if not exists author(
    id              INTEGER primary key autoincrement,
    name            TEXT not null unique
);

-- name: create_book_table!
create table if not exists book(
    id              TEXT primary key,
    title           TEXT,
    author_id       INTEGER references author(id) on delete set null,
    read_pos        TEXT                DEFAULT NULL,
    read_progress   REAL                DEFAULT 0,
    last_read       INTEGER             DEFAULT 0,
    added_at        INTEGER,
    -- Total version of the last add/delete that won this book's lifecycle
    -- state. The deletion rank is derived from deleted_at.
    hlc_pt          INTEGER not null default 0,
    hlc_l           INTEGER not null default 0,
    hlc_actor       TEXT not null default '',
    hlc_seq         INTEGER not null default 0,
    hlc_event       TEXT not null default '',
    deleted_at      INTEGER
);

-- name: create_book_search_document_table!
create table if not exists book_search_document(
    book_id          TEXT primary key references book(id) on delete cascade,
    normalized_text  TEXT not null
);

-- name: create_book_search_bigram_table!
create table if not exists book_search_bigram(
    book_id          TEXT not null references book(id) on delete cascade,
    gram             TEXT not null,
    primary key (book_id, gram)
);

-- name: create_book_search_bigram_index!
create index if not exists idx_book_search_bigram_gram
on book_search_bigram(gram, book_id);

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
    hlc_pt          INTEGER not null default 0,
    hlc_l           INTEGER not null default 0,
    hlc_actor       TEXT not null default '',
    hlc_seq         INTEGER not null default 0,
    hlc_event       TEXT not null default '',
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
    -- an LWW register. Local mutations stamp it via trig_event_log_hlc; remote
    -- applies gate on it (see apply_remote_book_dir) so concurrent operations
    -- converge even when their HLC components are equal.
    hlc_pt          INTEGER not null default 0,
    hlc_l           INTEGER not null default 0,
    hlc_actor       TEXT not null default '',
    hlc_seq         INTEGER not null default 0,
    hlc_event       TEXT not null default '',
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

-- name: create_event_log_table!
create table if not exists event_log(
    local_seq       INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id        TEXT not null default (
        lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-' ||
        lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' ||
        lower(hex(randomblob(6)))
    ),
    entity_id       TEXT not null,
    event_type      TEXT not null,
    payload         TEXT not null,
    timestamp       INTEGER not null,
    hlc_pt          INTEGER not null default 0,
    hlc_l           INTEGER not null default 0
);

-- name: create_event_log_event_id_index!
CREATE UNIQUE INDEX IF NOT EXISTS idx_event_log_event_id ON event_log(event_id);

-- name: create_event_log_singleton_guard_index!
CREATE INDEX IF NOT EXISTS idx_event_log_singleton_guard
ON event_log(entity_id, event_type, local_seq, hlc_pt, hlc_l);

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
