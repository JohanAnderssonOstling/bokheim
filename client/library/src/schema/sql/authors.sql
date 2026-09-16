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

