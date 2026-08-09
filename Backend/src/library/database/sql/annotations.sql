-- Typed local annotation persistence. Sync mutations are emitted by triggers.

-- name: get_annotations?
-- param: content_hash: &str
SELECT annotation.id, book.content_hash, anchor_kind, anchor_cfi, anchor_page, anchor_rects,
       anchor_fallback_cfi, exact_text, style, color, note,
       created_at, modified_at
FROM annotation JOIN book ON book.row_id=annotation.book_row_id
WHERE book.content_hash=:content_hash AND annotation.deleted_at IS NULL
ORDER BY modified_at DESC, annotation.id;

-- name: get_annotation_by_id?
-- param: id: &str
SELECT annotation.id, book.content_hash, anchor_kind, anchor_cfi, anchor_page, anchor_rects,
       anchor_fallback_cfi, exact_text, style, color, note,
       created_at, modified_at
FROM annotation JOIN book ON book.row_id=annotation.book_row_id
WHERE annotation.id=:id;

-- name: upsert_annotation!
-- param: id: &str
-- param: content_hash: &str
-- param: anchor_kind: &str
-- param: anchor_cfi: Option<&str>
-- param: anchor_page: Option<i64>
-- param: anchor_rects: Option<&[u8]>
-- param: anchor_fallback_cfi: Option<&str>
-- param: exact_text: &str
-- param: style: &str
-- param: color: &str
-- param: note: &str
-- param: created_at: i64
-- param: modified_at: i64
INSERT INTO annotation(
    id,book_row_id,anchor_kind,anchor_cfi,anchor_page,anchor_rects,
    anchor_fallback_cfi,exact_text,style,color,note,
    created_at,modified_at,deleted_at
)
VALUES(
    :id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:anchor_kind,:anchor_cfi,:anchor_page,:anchor_rects,
    :anchor_fallback_cfi,:exact_text,:style,:color,:note,
    :created_at,:modified_at,NULL
)
ON CONFLICT(id) DO UPDATE SET
    anchor_kind=excluded.anchor_kind,
    anchor_cfi=excluded.anchor_cfi,
    anchor_page=excluded.anchor_page,
    anchor_rects=excluded.anchor_rects,
    anchor_fallback_cfi=excluded.anchor_fallback_cfi,
    exact_text=excluded.exact_text,
    style=excluded.style,
    color=excluded.color,
    note=excluded.note,
    modified_at=excluded.modified_at,
    deleted_at=NULL;

-- name: delete_annotation!
-- param: id: &str
-- param: modified_at: i64
UPDATE annotation
SET modified_at=:modified_at, deleted_at=:modified_at
WHERE id=:id;
