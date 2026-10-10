-- name: schema_placements_tables &
-- Directory tree, the root directory, and book placement within it.
create table if not exists dir(
    id              TEXT primary key,
    parent_id       TEXT not null references dir(id) on delete cascade,
    name            TEXT not null CHECK(substr(name,1,1) != '.'),
    name_key        TEXT generated always as (portable_name_key(name)) stored,
    -- Authoritative synchronized register. The parent/name/deleted_at columns
    -- above are the derived, filesystem-safe projection and may temporarily
    -- suppress an otherwise-present intent to break cycles/path collisions.
    canonical_parent_id TEXT,
    canonical_name      TEXT,
    -- 0 = present, 1 = Trash, 2 = purge awaiting physical row deletion.
    -- Purge tombstones survive separately in sync_state_version.
    lifecycle INTEGER,
    deleted_at      INTEGER,
    CHECK (id != '00000000-0000-0000-0000-000000000000' OR deleted_at IS NULL)
);

create table if not exists book_dir(
    dir_id          TEXT not null references dir(id) on delete cascade,
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    file_name       TEXT not null,
    -- Per-machine filesystem fingerprint (nanosecond mtime/size/inode/dev). Cheap to
    -- compute on every scan so unchanged files can be skipped without re-reading
    -- their contents (see get_book_dir_by_hash). Not unique and not portable
    -- across devices, so it is an attribute, not the key.
    local_hash      TEXT not null,
    -- Native: this placement has materialized bytes. Browser: its shared OPFS object exists.
    is_downloaded   INTEGER not null default 0 CHECK(is_downloaded IN (0,1)),
    deleted_at      INTEGER,
    trash_origin_dir_id TEXT,
    -- Size of the locally verified bytes at this placement.
    size_bytes INTEGER CHECK(size_bytes > 0),
    -- A row is a file *placement*: one book in one directory.
    -- Same-path collisions remain local filesystem invariants enforced by
    -- idx_book_dir_live_path.
    PRIMARY KEY     (dir_id, book_row_id)
);
