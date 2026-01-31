-- name: create_book_table!
create table if not exists book(
    id              TEXT primary key,
    title           ,
    read_pos        TEXT                DEFAULT "0",
    read_progress   INTEGER             DEFAULT 0,
    last_read       INTEGER             DEFAULT 0,
    added_at        INTEGER,
    uploaded        INTEGER NOT NULL    DEFAULT 0,
    thumbnail_state INTEGER NOT NULL    DEFAULT 0
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
    event_kind      INTEGER not null
);

-- name: create_sync_metadata_table!
create table if not exists sync_metadata(
    key     TEXT primary key,
    value   TEXT not null
);
