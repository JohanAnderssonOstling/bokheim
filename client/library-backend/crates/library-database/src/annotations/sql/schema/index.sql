-- name: schema_annotations_index &
create index if not exists idx_annotation_book on annotation(book_row_id, deleted_at, toc_ordinal, progress);
