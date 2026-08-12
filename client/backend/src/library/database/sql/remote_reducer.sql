-- SQL used to materialize winning remote sync state. Merge ordering itself is
-- centralized in sync_state_version; these queries only update domain values.

-- name: upsert_remote_annotation!
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
-- param: deleted_at: Option<i64>
INSERT INTO annotation(id,book_row_id,anchor_kind,anchor_cfi,anchor_page,anchor_rects,anchor_fallback_cfi,exact_text,style,color,note,created_at,modified_at,deleted_at)
VALUES(:id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:anchor_kind,:anchor_cfi,:anchor_page,:anchor_rects,:anchor_fallback_cfi,:exact_text,:style,:color,:note,:created_at,:modified_at,:deleted_at)
ON CONFLICT(id) DO UPDATE SET
    book_row_id=excluded.book_row_id,anchor_kind=excluded.anchor_kind,anchor_cfi=excluded.anchor_cfi,
    anchor_page=excluded.anchor_page,anchor_rects=excluded.anchor_rects,
    anchor_fallback_cfi=excluded.anchor_fallback_cfi,exact_text=excluded.exact_text,
    style=excluded.style,color=excluded.color,
    note=excluded.note,created_at=excluded.created_at,modified_at=excluded.modified_at,
    deleted_at=excluded.deleted_at;

-- name: upsert_remote_bookmark!
-- param: id: &str
-- param: content_hash: &str
-- param: cfi: &str
-- param: label: &str
-- param: created_at: i64
-- param: modified_at: i64
-- param: deleted_at: Option<i64>
INSERT INTO bookmark(id,book_row_id,cfi,label,created_at,modified_at,deleted_at)
VALUES(:id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:cfi,:label,:created_at,:modified_at,:deleted_at)
ON CONFLICT(id) DO UPDATE SET
    modified_at=excluded.modified_at,deleted_at=excluded.deleted_at;

-- name: upsert_remote_book_link!
-- param: id: &str
-- param: source_content_hash: &str
-- param: target_content_hash: &str
-- param: relation: &str
-- param: note: &str
-- param: created_at: i64
-- param: modified_at: i64
-- param: deleted_at: Option<i64>
INSERT INTO book_link(id,source_book_row_id,target_book_row_id,relation,note,created_at,modified_at,deleted_at)
VALUES(:id,(SELECT row_id FROM book WHERE content_hash=:source_content_hash),(SELECT row_id FROM book WHERE content_hash=:target_content_hash),:relation,:note,:created_at,:modified_at,:deleted_at)
ON CONFLICT(id) DO UPDATE SET
    source_book_row_id=excluded.source_book_row_id,target_book_row_id=excluded.target_book_row_id,
    relation=excluded.relation,note=excluded.note,created_at=excluded.created_at,
    modified_at=excluded.modified_at,deleted_at=excluded.deleted_at;

-- name: set_remote_directory_name!
-- param: dir_id: &str
-- param: name: &str
UPDATE dir SET intent_name=:name WHERE id=:dir_id;

-- name: set_remote_directory_parent!
-- param: dir_id: &str
-- param: parent_id: &str
UPDATE dir SET intent_parent_id=:parent_id WHERE id=:dir_id;

-- name: set_remote_directory_lifecycle!
-- param: dir_id: &str
-- param: lifecycle: i32
UPDATE dir SET intent_lifecycle=:lifecycle WHERE id=:dir_id;

-- name: ensure_remote_directory_placeholder!
-- param: dir_id: &str
INSERT OR IGNORE INTO dir(id,parent_id,name,intent_parent_id,intent_name,intent_lifecycle,deleted_at,purged_at)
VALUES(:dir_id,'00000000-0000-0000-0000-000000000000','Pending','00000000-0000-0000-0000-000000000000','Pending',1,1,NULL);

-- name: get_remote_directory_intents?
SELECT id, COALESCE(intent_parent_id, parent_id), COALESCE(intent_name, name),
       COALESCE(intent_lifecycle, CASE WHEN purged_at IS NOT NULL THEN 2 WHEN deleted_at IS NOT NULL THEN 1 ELSE 0 END)
FROM dir
WHERE id != '00000000-0000-0000-0000-000000000000';

-- name: hide_remote_directories!
UPDATE dir SET deleted_at=1, purged_at=NULL WHERE id != '00000000-0000-0000-0000-000000000000';

-- name: mark_remote_directory_purged!
-- param: dir_id: &str
UPDATE dir SET deleted_at=1, purged_at=1 WHERE id=:dir_id;

-- name: project_remote_directory!
-- param: dir_id: &str
-- param: parent_id: &str
-- param: name: &str
UPDATE dir SET parent_id=:parent_id, name=:name WHERE id=:dir_id;

-- name: activate_remote_directory!
-- param: dir_id: &str
UPDATE dir SET deleted_at=NULL, purged_at=NULL WHERE id=:dir_id;

-- name: remote_book_has_metadata?
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=:content_hash AND title IS NOT NULL);
