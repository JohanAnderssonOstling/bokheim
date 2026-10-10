-- Reader annotations: the whole annotation is the versioned binary `body`;
-- book, ordering and deletion are copied into columns to join, sort and
-- filter on. Projection writes these rows from canonical registers.

-- name: get_annotations?
-- param: content_hash: &ContentHash
SELECT annotation.body
FROM annotation JOIN book ON book.row_id=annotation.book_row_id
WHERE book.content_hash=:content_hash AND annotation.deleted_at IS NULL
ORDER BY annotation.toc_ordinal NULLS LAST, annotation.progress NULLS LAST,
         annotation.modified_at DESC, annotation.id;

-- name: get_annotation_by_id?
-- param: id: &str
SELECT annotation.body FROM annotation WHERE annotation.id=:id;

-- name: upsert_annotation!
-- param: id: &str
-- param: content_hash: &ContentHash
-- param: toc_ordinal: Option<i64>
-- param: progress: Option<f32>
-- param: body: &[u8]
-- param: modified_at: i64
-- param: deleted_at: Option<i64>
INSERT INTO annotation(
    id,book_row_id,toc_ordinal,progress,body,
    modified_at,deleted_at
)
VALUES(
    :id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:toc_ordinal,:progress,:body,
    :modified_at,:deleted_at
)
ON CONFLICT(id) DO UPDATE SET
    book_row_id=excluded.book_row_id,
    toc_ordinal=excluded.toc_ordinal,
    progress=excluded.progress,
    body=excluded.body,
    modified_at=excluded.modified_at,
    deleted_at=excluded.deleted_at;


-- name: annotation_book_exists ->
-- param: content_hash: &SqlParam<'_>
SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=:content_hash);
