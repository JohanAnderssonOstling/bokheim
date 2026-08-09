-- Typed local annotation persistence. Sync mutations are emitted by triggers.

-- name: get_annotations?
-- param: book_id: &str
SELECT id, book_id, anchor_kind, anchor_cfi, anchor_page, anchor_rects,
       anchor_fallback_cfi, exact_text, style, color, note,
       created_at, modified_at
FROM annotation
WHERE book_id=:book_id AND deleted_at IS NULL
ORDER BY modified_at DESC, id;

-- name: get_annotation_by_id?
-- param: id: &str
SELECT id, book_id, anchor_kind, anchor_cfi, anchor_page, anchor_rects,
       anchor_fallback_cfi, exact_text, style, color, note,
       created_at, modified_at
FROM annotation
WHERE id=:id;

-- name: upsert_annotation!
-- param: id: &str
-- param: book_id: &str
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
    id,book_id,anchor_kind,anchor_cfi,anchor_page,anchor_rects,
    anchor_fallback_cfi,exact_text,style,color,note,
    created_at,modified_at,deleted_at
)
VALUES(
    :id,:book_id,:anchor_kind,:anchor_cfi,:anchor_page,:anchor_rects,
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
