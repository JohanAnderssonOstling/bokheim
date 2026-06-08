create table if not exists users (
    id text primary key,
    created_at timestamptz not null default now()
);

create table if not exists libraries (
    id text primary key,
    user_id text not null references users(id) on delete cascade,
    created_at timestamptz not null default now()
);

-- Create library_events with the final schema (no id/timestamp columns)
create table if not exists library_events (
    library_id text not null references libraries(id) on delete cascade,
    user_id text not null references users(id) on delete cascade,
    event_type text not null,
    entity_id text not null,
    data jsonb not null,
    client_id text not null,
    timestamp bigint not null,
    client_seq bigint not null default 0,
    hlc_pt bigint not null default 0,
    hlc_l bigint not null default 0,
    server_seq bigserial primary key
);

-- legacy columns from earlier schema iterations
alter table library_events drop column if exists event_kind;
alter table library_events drop column if exists hlc_node;

create index if not exists idx_library_events_library_seq
    on library_events(library_id, server_seq);

-- The two event_type lists below must match sync_common::SINGLETON_EVENT_TYPES.
-- If you add a new singleton type, add it to both predicates AND the Rust const.
create unique index if not exists idx_library_events_lww
    on library_events(library_id, entity_id, event_type)
    where event_type in ('reading_position_updated', 'book_opened');

create unique index if not exists idx_library_events_dedup
    on library_events(library_id, client_id, client_seq)
    where event_type not in ('reading_position_updated', 'book_opened');
