-- name: schema_sync_index &
create index if not exists idx_book_language_value on book_language(language, book_row_id);

create index if not exists idx_book_contributor_identity on book_contributor(author_identity_id, book_row_id);

create index if not exists idx_book_subject_name on book_subject(name, book_row_id);

create index if not exists idx_book_subject_authority on book_subject(authority, book_row_id);


create index if not exists idx_book_unified_concept_concept on book_unified_concept(concept_id,book_row_id);

create index if not exists idx_author_identity_provisional
on author_identity(is_provisional);

CREATE INDEX IF NOT EXISTS sync_state_book ON sync_state_version(book_key);
