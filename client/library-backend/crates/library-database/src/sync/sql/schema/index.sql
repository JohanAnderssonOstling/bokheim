-- name: schema_sync_index &
create index if not exists idx_book_identifier_lookup on book_identifier(scheme, value, book_row_id);

create index if not exists idx_book_language_value on book_language(language, book_row_id);

create index if not exists idx_book_contributor_identity on book_contributor(author_identity_id, book_row_id);

create index if not exists idx_book_subject_name on book_subject(name, book_row_id);

create index if not exists idx_book_subject_authority on book_subject(authority, book_row_id);


create index if not exists idx_book_unified_concept_concept on book_unified_concept(concept_id,book_row_id);

create unique index if not exists idx_author_identity_stable_id
on author_identity(stable_id);

create index if not exists idx_author_identity_normalized_name
on author_identity(normalized_name, is_provisional);

CREATE INDEX IF NOT EXISTS sync_state_book ON sync_state_version(book_key);
