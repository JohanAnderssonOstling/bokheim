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
        'annotation', 'catalog_publication', 'metadata', 'description', 'pdf_reader_metadata'
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

create table if not exists sync_metadata(
    key     TEXT primary key,
    value   TEXT not null
);

-- Assets confirmed to exist remotely. Absence means unknown, not missing.
