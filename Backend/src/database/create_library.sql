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
    read_pos        TEXT                DEFAULT "0",
    read_progress   INTEGER             DEFAULT 0,
    last_read       INTEGER             DEFAULT 0,
    added_at        INTEGER,
    deleted_at      INTEGER
);

-- name: create_dir_table!
create table if not exists dir(
    id              TEXT primary key,
    parent_id       TEXT not null references dir(id) on delete cascade,
    name            TEXT not null,
    deleted_at      INTEGER
);

-- name: insert_root!
insert into dir(id, parent_id, name, deleted_at)
values ('__root__', '__root__', '', NULL)
on conflict(id) do update set parent_id = excluded.parent_id, name = excluded.name, deleted_at = NULL;

-- name: create_book_dir_table!
create table if not exists book_dir(
    dir_id          TEXT not null references dir(id) on delete cascade,
    book_id         TEXT not null references book(id) on delete cascade,
    file_name       TEXT not null,
    local_hash      TEXT not null,
    is_downloaded   INTEGER not null default 0,
    deleted_at      INTEGER,
    PRIMARY KEY     (local_hash)
);

-- name: create_event_log_table!
create table if not exists event_log(
    local_seq       INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_id       TEXT not null,
    event_type      TEXT not null,
    payload         TEXT not null,
    timestamp       INTEGER not null,
    hlc_pt          INTEGER not null default 0,
    hlc_l           INTEGER not null default 0,
    event_kind      INTEGER not null
);

-- name: create_sync_metadata_table!
create table if not exists sync_metadata(
    key     TEXT primary key,
    value   TEXT not null
);

-- name: create_blob_state_table!
create table if not exists blob_state(
    kind            TEXT not null,         -- 'book' | 'thumbnail'
    hash            TEXT not null,
    state           INTEGER not null,      -- 0 unknown, 1 local-present, 2 verified-on-server, 3 server-only
    last_checked    INTEGER,
    last_error      TEXT,
    primary key (kind, hash)
);

-- name: create_blob_local_ref_table!
create table if not exists blob_local_ref(
    kind            TEXT not null,         -- 'book' | 'thumbnail'
    hash            TEXT not null,
    path            TEXT not null,
    exists_flag     INTEGER not null default 0,
    last_seen       INTEGER not null default 0,
    primary key (kind, hash, path)
);

-- name: create_transfer_jobs_table!
create table if not exists transfer_jobs(
    id              INTEGER primary key autoincrement,
    kind            TEXT not null,
    key1            TEXT not null,
    rel_path        TEXT not null default '',
    local_path      TEXT not null default '',
    book_id         TEXT not null default '',
    attempts        INTEGER not null default 0,
    claimed_at      INTEGER,
    next_retry_at   INTEGER not null default 0,
    last_error      TEXT,
    created_at      INTEGER not null,
    updated_at      INTEGER not null,
    unique(kind, key1, rel_path, local_path, book_id)
);
