-- Book rows, publication metadata projection, and subject taxonomy.
create table if not exists book(
    row_id          INTEGER primary key,
    -- Portable identity used by sync and asset APIs. Local relationships use
    -- row_id so their indexes remain compact and integer joins stay cheap.
    content_hash    TEXT not null unique,
    title           TEXT,
    description_scanned INTEGER not null default 0,
    -- Canonical synchronized bibliographic metadata. Dedicated tables below
    -- are disposable query projections rebuilt from this value.
    publication_metadata BLOB not null default X'',
    -- Canonical extension of the book's format ('epub', 'pdf', ...). Fixed for
    -- the life of the row: identity is derived from the bytes, so the format
    -- cannot change without becoming a different book. Empty only on a row that
    -- has never been observed on any device's disk.
    format          TEXT not null default '',
    -- Device-local library visibility. This is intentionally not synchronized.
    hidden_at       INTEGER,
    read_pos        TEXT                DEFAULT NULL,
    read_progress   REAL                DEFAULT 0,
    added_at        INTEGER,
    deleted_at      INTEGER,
    purged_at       INTEGER,
    trash_origin_dir_id TEXT,
    CHECK (purged_at IS NULL OR deleted_at IS NOT NULL)
);

create table if not exists publisher(
    id              INTEGER primary key,
    name            TEXT not null,
    normalized_name TEXT not null unique
);
create table if not exists book_publisher(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    publisher_id    INTEGER not null references publisher(id) on delete cascade,
    agent_identity_id INTEGER not null references author_identity(id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    credited_name   TEXT not null,
    primary key(book_row_id, position)
);
create index if not exists idx_book_publisher_publisher on book_publisher(publisher_id, book_row_id);
create index if not exists idx_book_publisher_agent on book_publisher(agent_identity_id, book_row_id);
create table if not exists book_identifier(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    scheme          TEXT not null,
    value           TEXT not null,
    canonical_value TEXT,
    source          TEXT not null,
    scope           TEXT not null default 'publication' check(scope in ('publication','edition','work')),
    primary key(book_row_id, position)
);
create index if not exists idx_book_identifier_lookup on book_identifier(scheme, value, book_row_id);
create table if not exists book_language(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    language_tag    TEXT not null,
    primary key(book_row_id, position)
);
create index if not exists idx_book_language_tag on book_language(language_tag, book_row_id);
-- Merged relational contributor register. Every role (including "aut" =
-- author) shares one flat per-book position sequence. This replaces the
-- former separate book_author table; author_credit (see views.sql) exposes
-- the author-only subset for call sites that only care about authorship.
create table if not exists book_contributor(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    author_identity_id INTEGER not null references author_identity(id) on delete cascade,
    name            TEXT not null,
    sort_name       TEXT,
    role            BLOB not null check(typeof(role) = 'blob' and length(role) = 3),
    primary key(book_row_id, position)
);
create index if not exists idx_book_contributor_identity on book_contributor(author_identity_id, book_row_id);
create table if not exists book_subject(
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    position        INTEGER not null check(position >= 0),
    name            TEXT not null,
    source          TEXT not null,
    authority       TEXT,
    code            TEXT,
    primary key(book_row_id, position)
);
create index if not exists idx_book_subject_name on book_subject(name, book_row_id);
create index if not exists idx_book_subject_authority on book_subject(authority, book_row_id);
create table if not exists book_subject_code(
    book_row_id     INTEGER not null,
    subject_position INTEGER not null,
    system_id       TEXT not null,
    code            TEXT not null,
    primary key(book_row_id, subject_position, system_id, code),
    foreign key(book_row_id, subject_position) references book_subject(book_row_id, position) on delete cascade
);
create index if not exists idx_book_subject_code_lookup on book_subject_code(system_id, code, book_row_id);

-- Controlled taxonomies and disposable automatic assignments. The raw
-- declarations above remain the evidence and synchronization boundary.
create table if not exists subject_system(
    id              TEXT primary key,
    version         TEXT not null,
    matcher_version INTEGER not null check(matcher_version >= 0)
);
create table if not exists subject_node(
    system_id       TEXT not null references subject_system(id) on delete cascade,
    path            TEXT not null,
    parent_path     TEXT not null,
    name            TEXT not null,
    code            TEXT,
    assignable      INTEGER not null check(assignable in (0, 1)),
    primary key(system_id, path),
    unique(system_id, code)
);
create index if not exists idx_subject_node_parent on subject_node(system_id, parent_path, name);
create table if not exists book_subject_assignment(
    book_row_id     INTEGER not null references book(row_id) on delete cascade,
    system_id       TEXT not null,
    subject_path    TEXT not null,
    matcher_version INTEGER not null check(matcher_version >= 0),
    primary key(book_row_id, system_id, subject_path),
    foreign key(system_id, subject_path) references subject_node(system_id, path) on delete cascade
);
create index if not exists idx_book_subject_assignment_subject on book_subject_assignment(system_id, subject_path, book_row_id);
create table if not exists book_subject_assignment_evidence(
    book_row_id     INTEGER not null,
    system_id       TEXT not null,
    subject_path    TEXT not null,
    subject_position INTEGER not null,
    primary key(book_row_id, system_id, subject_path, subject_position),
    foreign key(book_row_id, system_id, subject_path) references book_subject_assignment(book_row_id, system_id, subject_path) on delete cascade,
    foreign key(book_row_id, subject_position) references book_subject(book_row_id, position) on delete cascade
);
create table if not exists subject_code_mapping(
    source_system_id TEXT not null,
    source_code      TEXT not null,
    unified_path     TEXT not null,
    mapper_version   INTEGER not null check(mapper_version >= 0),
    primary key(source_system_id, source_code, unified_path)
);
create index if not exists idx_subject_code_mapping_path on subject_code_mapping(unified_path, source_system_id, source_code);

-- Curated subjects and navigation live in the attached read-only taxonomy.
-- Library storage contains book assignments and their evidence only.
create table if not exists source_subject(
    system_id       TEXT not null,
    source_version  TEXT not null,
    code            TEXT not null,
    official_label  TEXT,
    primary key(system_id,source_version,code)
);
create table if not exists source_concept_mapping(
    source_system_id TEXT not null,
    source_version   TEXT not null,
    source_code      TEXT not null,
    concept_id INTEGER not null check(concept_id > 0),
    mapping_type     TEXT not null check(mapping_type in ('exact','broader','narrower','related')),
    mapper_version   INTEGER not null check(mapper_version >= 0),
    primary key(source_system_id,source_version,source_code,concept_id),
    foreign key(source_system_id,source_version,source_code) references source_subject(system_id,source_version,code) on delete cascade
);
create index if not exists idx_source_concept_mapping_concept on source_concept_mapping(concept_id,source_system_id,source_code);
create table if not exists book_unified_concept(
    book_row_id     INTEGER not null references book(row_id) on delete cascade,
    concept_id INTEGER not null check(concept_id > 0),
    mapper_version  INTEGER not null check(mapper_version >= 0),
    primary key(book_row_id,concept_id)
);
create index if not exists idx_book_unified_concept_concept on book_unified_concept(concept_id,book_row_id);
create table if not exists book_unified_concept_evidence(
    book_row_id      INTEGER not null,
    concept_id INTEGER not null,
    subject_position INTEGER not null,
    source_system_id TEXT not null,
    source_code      TEXT not null,
    primary key(book_row_id,concept_id,subject_position,source_system_id,source_code),
    foreign key(book_row_id,concept_id) references book_unified_concept(book_row_id,concept_id) on delete cascade,
    foreign key(book_row_id,subject_position,source_system_id,source_code) references book_subject_code(book_row_id,subject_position,system_id,code) on delete cascade
);

-- Local bookkeeping for explicit external enrichment. Provider vocabularies
-- are never cached here; this records only whether one book was attempted.
