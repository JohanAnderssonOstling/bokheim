-- External metadata attempts and the entities they resolve to.
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
-- Local cache of resolved metadata-server entities. Book references are also
-- projected into synchronized PublicationMetadata; labels and graph edges can
-- be rebuilt independently on every client from the same snapshot service.
create table if not exists external_entity(
    wikidata_id    TEXT primary key,
    kind           TEXT not null check(kind in ('place', 'person')),
    label          TEXT not null,
    birth_year     INTEGER,
    death_year     INTEGER,
    fictional      INTEGER not null default 0 check(fictional in (0, 1)),
    latitude_e7    INTEGER,
    longitude_e7   INTEGER
);
create table if not exists external_entity_alias(
    wikidata_id    TEXT not null references external_entity(wikidata_id) on delete cascade,
    alias          TEXT not null,
    primary key(wikidata_id, alias)
);
create table if not exists external_place_edge(
    child_id       TEXT not null references external_entity(wikidata_id) on delete cascade,
    parent_id      TEXT not null references external_entity(wikidata_id) on delete cascade,
    relation       TEXT not null check(relation in ('parent', 'country')),
    primary key(child_id, parent_id, relation)
);
create index if not exists idx_external_place_edge_parent on external_place_edge(parent_id, child_id);
create table if not exists book_external_entity(
    book_row_id    INTEGER not null references book(row_id) on delete cascade,
    wikidata_id    TEXT not null references external_entity(wikidata_id) on delete cascade,
    kind           TEXT not null check(kind in ('place', 'person')),
    evidence_name  TEXT not null,
    source         TEXT not null check(source in ('exact_edition', 'work')),
    primary key(book_row_id, wikidata_id, kind, evidence_name)
);
create index if not exists idx_book_external_entity_lookup on book_external_entity(kind, wikidata_id, book_row_id);
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
