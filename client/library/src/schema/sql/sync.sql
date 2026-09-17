-- Durable synchronization outbox, winning versions, and replica metadata.
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
        'annotation', 'metadata', 'description', 'pdf_reader_metadata',
        'book_toc'
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

-- Replica metadata lives in one singleton row with one explicit column per
-- value. There is intentionally no key/value pair: every durable value must
-- be declared here so its storage type is visible in the schema.
create table if not exists sync_metadata(
    singleton                   INTEGER primary key check(singleton = 1),
    replica_id                  TEXT,
    change_origin               TEXT not null default 'local' check(change_origin in ('local', 'remote')),
    last_successful_sync_at     INTEGER check(last_successful_sync_at is null or last_successful_sync_at >= 0),
    last_pull_state_seq         INTEGER check(last_pull_state_seq is null or last_pull_state_seq >= 0),
    last_pull_reading_seq       INTEGER check(last_pull_reading_seq is null or last_pull_reading_seq >= 0),
    inventory_repair_kind       TEXT,
    inventory_repair_key        TEXT,
    inventory_repair_subkey     TEXT,
    cursor_recovery_pending     INTEGER check(cursor_recovery_pending is null or cursor_recovery_pending in (0, 1)),
    scan_seq                    INTEGER not null default 0 check(scan_seq >= 0),
    operation_id                TEXT,
    operation_kind              TEXT check(operation_kind is null or operation_kind in ('Import', 'Sync')),
    operation_scan_complete     INTEGER check(operation_scan_complete is null or operation_scan_complete in (0, 1)),
    operation_scan_failed       INTEGER check(operation_scan_failed is null or operation_scan_failed in (0, 1)),
    operation_discovered        INTEGER check(operation_discovered is null or operation_discovered >= 0),
    operation_failures          INTEGER check(operation_failures is null or operation_failures >= 0),
    operation_requires_sync     INTEGER check(operation_requires_sync is null or operation_requires_sync in (0, 1)),
    operation_completed         INTEGER check(operation_completed is null or operation_completed in (0, 1)),
    operation_sync_unfinished   INTEGER check(operation_sync_unfinished is null or operation_sync_unfinished in (0, 1)),
    cloud_storage_enabled       INTEGER check(cloud_storage_enabled is null or cloud_storage_enabled in (0, 1)),
    remote_library_name         TEXT,
    server_library_created      TEXT
);
insert or ignore into sync_metadata(singleton) values(1);

-- Session-local UI history is not synchronization metadata. It has one
-- durable value per library and therefore has its own table.
create table if not exists local_transfer_history(
    singleton    INTEGER primary key check(singleton = 1),
    entries_json TEXT not null
);

-- Assets confirmed to exist remotely. Absence means unknown, not missing.
