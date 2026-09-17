-- Resolves local or synchronized book placements.

-- name: placements_upsert_dir!
-- param: id: &str
-- param: parent_id: &str
-- param: name: &str
INSERT INTO dir (id, parent_id, name, intent_parent_id, intent_name, intent_lifecycle, deleted_at, purged_at)
VALUES (:id, :parent_id, :name, :parent_id, :name, 0, NULL, NULL)
ON CONFLICT(id) DO UPDATE SET
    parent_id = excluded.parent_id,
    name = excluded.name,
    intent_parent_id = excluded.parent_id,
    intent_name = excluded.name,
    deleted_at = NULL,
    purged_at = NULL;

-- name: placements_live_directory_exists?
-- param: directory_id: &str
SELECT EXISTS(
    SELECT 1 FROM dir WHERE id=:directory_id AND deleted_at IS NULL
);

-- name: placements_dir_path_components?
-- param: dir_id: &str
WITH RECURSIVE path_chain(id, parent_id, name, level) AS (
    SELECT id, parent_id, name, 0 AS level
    FROM dir
    WHERE id = :dir_id AND deleted_at IS NULL
    UNION ALL
    SELECT d.id, d.parent_id, d.name, pc.level + 1
    FROM dir d
    JOIN path_chain pc ON pc.parent_id = d.id
    WHERE d.deleted_at IS NULL AND d.id != '00000000-0000-0000-0000-000000000000'
)
SELECT id, name
FROM path_chain
WHERE id != '00000000-0000-0000-0000-000000000000'
ORDER BY level DESC;

-- name: set_book_hash_downloaded!
-- param: content_hash: &str
-- param: is_downloaded: i32
UPDATE book_dir
SET is_downloaded = :is_downloaded
WHERE book_row_id = (SELECT row_id FROM book WHERE content_hash=:content_hash)
  AND deleted_at IS NULL;

-- name: placements_directory_move_closes_cycle?
-- param: parent_id: &str
-- param: directory_id: &str
WITH RECURSIVE ancestors(id) AS (
    SELECT :parent_id
    UNION
    SELECT d.parent_id
    FROM dir d JOIN ancestors ON d.id=ancestors.id
    WHERE d.id!='00000000-0000-0000-0000-000000000000'
)
SELECT EXISTS(SELECT 1 FROM ancestors WHERE id=:directory_id);

-- name: placements_is_live_destination?
-- param: directory_id: &str
SELECT EXISTS(SELECT 1 FROM dir_paths WHERE id=:directory_id);

-- name: placements_directory_is_rooted?
-- param: directory_id: &str
-- param: root_id: &str
SELECT parent_id = :root_id FROM dir WHERE id = :directory_id;

-- name: placements_live_file_name?
-- param: directory_id: &str
-- param: content_hash: &str
SELECT file_name FROM book_dir
WHERE dir_id = :directory_id
  AND book_row_id = (SELECT row_id FROM book WHERE content_hash = :content_hash)
  AND deleted_at IS NULL;

-- name: placements_folder_name_occupied?
-- param: parent_id: &str
-- param: name_key: &str
-- param: excluded_id: &str
SELECT EXISTS(SELECT 1 FROM dir WHERE parent_id=:parent_id AND name_key=:name_key AND deleted_at IS NULL AND id!='00000000-0000-0000-0000-000000000000' AND id!=:excluded_id);

-- name: placements_sibling_folder_names?
-- param: parent_id: &str
-- param: excluded_id: &str
SELECT name FROM dir WHERE parent_id=:parent_id AND id!=:excluded_id AND deleted_at IS NULL;

-- name: placements_file_name_occupied?
-- param: directory_id: &str
-- param: name_key: &str
SELECT EXISTS(SELECT 1 FROM book_dir WHERE dir_id=:directory_id AND portable_name_key(file_name)=:name_key AND deleted_at IS NULL);

-- name: placements_book_is_trashed?
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=:content_hash AND deleted_at IS NOT NULL AND purged_at IS NULL);

-- name: placements_queue_directory_work!
-- param: dir_id: &str
INSERT OR REPLACE INTO local_directory_work(dir_id) VALUES(:dir_id);

-- name: placements_queue_file_work!
-- param: operation: &str
-- param: content_hash: &str
-- param: relative_path: &str
INSERT OR IGNORE INTO local_file_work(operation,content_hash,relative_path) VALUES(:operation,:content_hash,:relative_path);

-- name: placements_copy_book_baseline!
-- param: source_dir_id: &str
-- param: destination_dir_id: &str
-- param: content_hash: &str
INSERT INTO local_book_current(dir_id,content_hash,checksum,origin)
SELECT :destination_dir_id,content_hash,checksum,origin FROM local_book_current WHERE dir_id=:source_dir_id AND content_hash=:content_hash
ON CONFLICT(dir_id,content_hash) DO UPDATE SET checksum=excluded.checksum,origin=excluded.origin;

-- name: placements_occupied_file_names?
-- param: destination: &str
SELECT file_name FROM book_dir WHERE dir_id = :destination AND deleted_at IS NULL;

-- name: placements_folder_destinations?
WITH RECURSIVE tree(id,parent_id,label,path) AS (
    SELECT id,NULL,'Library','' FROM dir WHERE id='00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
    UNION ALL
    SELECT child.id,child.parent_id,child.name,CASE WHEN tree.path='' THEN child.name ELSE tree.path||' / '||child.name END
    FROM dir child JOIN tree ON child.parent_id=tree.id
    WHERE child.id!=child.parent_id AND child.deleted_at IS NULL
)
SELECT id,parent_id,label,path FROM tree ORDER BY portable_name_key(path),id;

-- name: placements_add_download_request!
-- param: target_kind: &str
-- param: target_key: &str
-- param: origin: &str
-- param: created_at: i64
INSERT INTO download_requests(target_kind, target_key, origin, created_at)
VALUES (:target_kind, :target_key, :origin, :created_at)
ON CONFLICT(target_kind, target_key) DO UPDATE SET
    origin = CASE WHEN excluded.origin = 'user_initiated' THEN 'user_initiated' ELSE download_requests.origin END;


-- name: cancel_book_restore!
-- param: content_hash: &str
DELETE FROM local_file_work WHERE content_hash=:content_hash AND operation='restore';

-- name: copy_directory_record_insert!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
-- param: is_downloaded: i32
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,deleted_at,trash_origin_dir_id) VALUES(:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,'',:is_downloaded,NULL,NULL);

-- name: copy_directory_record_select?
-- param: dir_id: &str
SELECT name FROM dir WHERE id=:dir_id AND deleted_at IS NULL;

-- name: copy_directory_record_select_2?
-- param: parent_id: &str
SELECT name FROM dir WHERE parent_id=:parent_id AND deleted_at IS NULL;

-- name: copy_directory_record_select_3?
-- param: dir_id: &str
SELECT b.content_hash,bd.file_name,bd.is_downloaded FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.dir_id=:dir_id AND bd.deleted_at IS NULL AND b.deleted_at IS NULL;

-- name: copy_directory_record_select_4?
-- param: parent_id: &str
SELECT id,name FROM dir WHERE parent_id=:parent_id AND id!=parent_id AND deleted_at IS NULL;

-- name: create_directory_with_id_select?
-- param: dir_id: &str
SELECT parent_id,name,deleted_at FROM dir WHERE id=:dir_id;

-- name: empty_trash_records_select?
SELECT content_hash FROM book WHERE deleted_at IS NOT NULL AND purged_at IS NULL;

-- name: empty_trash_records_with!
-- param: now: i64
WITH RECURSIVE subtree(id) AS (
             SELECT id FROM dir WHERE deleted_at IS NOT NULL AND purged_at IS NULL AND intent_lifecycle=1
             UNION SELECT child.id FROM dir child JOIN subtree ON child.parent_id=subtree.id
         ) UPDATE dir SET deleted_at=:now,purged_at=:now,intent_lifecycle=2 WHERE id IN (SELECT id FROM subtree);

-- name: library_trash_select?
SELECT content_hash,COALESCE(NULLIF(title,''),content_hash),format,deleted_at
         FROM book WHERE deleted_at IS NOT NULL AND purged_at IS NULL
         ORDER BY deleted_at DESC,portable_name_key(COALESCE(title,content_hash)),content_hash;

-- name: library_trash_select_2?
SELECT id,name,deleted_at FROM dir WHERE deleted_at IS NOT NULL AND purged_at IS NULL AND intent_lifecycle=1 ORDER BY deleted_at DESC,name_key,id;

-- name: move_directory_record_select?
-- param: dir_id: &str
SELECT parent_id,name FROM dir WHERE id=:dir_id AND deleted_at IS NULL;

-- name: purge_annotation!
-- param: book_row_id: i64
DELETE FROM annotation WHERE book_row_id=:book_row_id;

-- name: purge_book_collection!
-- param: book_row_id: i64
DELETE FROM book_collection WHERE book_row_id=:book_row_id;

-- name: purge_book_contributor!
-- param: book_row_id: i64
DELETE FROM book_contributor WHERE book_row_id=:book_row_id;

-- name: purge_book_external_entity!
-- param: book_row_id: i64
DELETE FROM book_external_entity WHERE book_row_id=:book_row_id;

-- name: purge_book_identifier!
-- param: book_row_id: i64
DELETE FROM book_identifier WHERE book_row_id=:book_row_id;

-- name: purge_book_language!
-- param: book_row_id: i64
DELETE FROM book_language WHERE book_row_id=:book_row_id;

-- name: purge_book_date!
-- param: book_row_id: i64
DELETE FROM book_date WHERE book_row_id=:book_row_id;

-- name: purge_book_value!
-- param: book_row_id: i64
DELETE FROM book_value WHERE book_row_id=:book_row_id;

-- name: purge_book_publisher!
-- param: book_row_id: i64
DELETE FROM book_publisher WHERE book_row_id=:book_row_id;

-- name: purge_book_record_delete_2!
-- param: content_hash: &str
DELETE FROM local_thumbnail_work WHERE content_hash=:content_hash;

-- name: purge_book_record_delete_3!
-- param: book_row_id: i64
DELETE FROM book_dir WHERE book_row_id=:book_row_id;

-- name: purge_book_record_delete_4!
-- param: content_hash: &str
DELETE FROM audiobook_metadata WHERE content_hash=:content_hash;

-- name: purge_book_record_delete_5!
-- param: target_key: &str
DELETE FROM download_requests WHERE target_key=:target_key;

-- name: purge_book_record_delete_6!
-- param: hash: &str
DELETE FROM remote_asset WHERE hash=:hash;

-- name: purge_book_record_delete_7!
-- param: book_row_id: i64
DELETE FROM book_description WHERE book_row_id=:book_row_id;

-- name: purge_book_record_insert!
-- param: content_hash: &str
INSERT OR IGNORE INTO local_purge_work(content_hash) VALUES(:content_hash);

-- name: purge_book_record_select?
-- param: content_hash: &str
SELECT row_id FROM book WHERE content_hash=:content_hash AND deleted_at IS NOT NULL AND purged_at IS NULL;

-- name: purge_book_record_update!
-- param: book_row_id: i64
-- param: now: i64
UPDATE book_dir SET deleted_at=:now,trash_origin_dir_id=NULL WHERE book_row_id=:book_row_id AND deleted_at IS NULL;

-- name: purge_book_record_update_2!
-- param: book_row_id: i64
-- param: now: i64
UPDATE book SET title=NULL,description_scanned=0,book_metadata=X'',format='',hidden_at=NULL,read_pos=NULL,read_progress=0,added_at=NULL,deleted_at=:now,purged_at=:now,trash_origin_dir_id=NULL WHERE row_id=:book_row_id;

-- name: purge_book_subject!
-- param: book_row_id: i64
DELETE FROM book_subject WHERE book_row_id=:book_row_id;

-- name: purge_book_subject_assignment!
-- param: book_row_id: i64
DELETE FROM book_subject_assignment WHERE book_row_id=:book_row_id;

-- name: purge_book_subject_assignment_evidence!
-- param: book_row_id: i64
DELETE FROM book_subject_assignment_evidence WHERE book_row_id=:book_row_id;

-- name: purge_directory_record_with!
-- param: dir_id: &str
-- param: now: i64
WITH RECURSIVE subtree(id) AS (SELECT id FROM dir WHERE id=:dir_id AND deleted_at IS NOT NULL AND purged_at IS NULL UNION ALL SELECT child.id FROM dir child JOIN subtree ON child.parent_id=subtree.id WHERE child.id!=child.parent_id) UPDATE dir SET deleted_at=:now,purged_at=:now,intent_lifecycle=2 WHERE id IN (SELECT id FROM subtree);

-- name: purge_external_metadata_attempt!
-- param: book_row_id: i64
DELETE FROM external_metadata_attempt WHERE book_row_id=:book_row_id;

-- name: remove_book_placement_record_select?
-- param: dir_id: &str
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id WHERE bd.dir_id=:dir_id AND book.content_hash=:content_hash AND bd.deleted_at IS NULL AND book.deleted_at IS NULL);

-- name: remove_book_placement_record_select_2?
-- param: content_hash: &str
-- param: dir_id: &str
SELECT NOT EXISTS(SELECT 1 FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND dir_id!=:dir_id AND deleted_at IS NULL);

-- name: remove_book_placement_record_update!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: deleted_at: i64
UPDATE book_dir SET deleted_at=:deleted_at,is_downloaded=0,trash_origin_dir_id=NULL WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND deleted_at IS NULL;

-- name: restore_book_placement_record_insert!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,deleted_at,trash_origin_dir_id) VALUES(:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,'',0,NULL,NULL) ON CONFLICT(dir_id,book_row_id) DO UPDATE SET file_name=excluded.file_name,deleted_at=NULL,trash_origin_dir_id=NULL;

-- name: restore_book_placement_record_select?
-- param: dir_id: &str
-- param: content_hash: &str
SELECT file_name FROM book_dir WHERE dir_id=:dir_id AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) ORDER BY deleted_at IS NULL DESC, deleted_at DESC LIMIT 1;

-- name: restore_book_placement_record_select_2?
-- param: content_hash: &str
SELECT COALESCE(NULLIF(title,''),content_hash) FROM book WHERE content_hash=:content_hash;

-- name: restore_book_placement_record_select_3?
-- param: dir_id: &str
-- param: content_hash: &str
SELECT file_name FROM book_dir WHERE dir_id=:dir_id AND deleted_at IS NULL AND book_row_id!=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: restore_book_placement_record_update!
-- param: content_hash: &str
UPDATE book SET deleted_at=NULL,purged_at=NULL,trash_origin_dir_id=NULL WHERE content_hash=:content_hash AND purged_at IS NULL;

-- name: restore_book_record_with_parent_insert!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,last_scan) VALUES(:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,'',0,0) ON CONFLICT(dir_id,book_row_id) DO UPDATE SET deleted_at=NULL,trash_origin_dir_id=NULL,file_name=excluded.file_name,is_downloaded=0,last_scan=0,local_hash='';

-- name: restore_book_record_with_parent_select_2?
-- param: content_hash: &str
SELECT dir_id,file_name FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND (deleted_at IS NULL OR deleted_at=(SELECT MAX(deleted_at) FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash))) ORDER BY dir_id;

-- name: restore_book_record_with_parent_select_3?
-- param: content_hash: &str
SELECT trash_origin_dir_id FROM book WHERE content_hash=:content_hash;

-- name: restore_book_record_with_parent_select_4?
-- param: dir_id: &str
SELECT parent_id,deleted_at IS NULL AND purged_at IS NULL FROM dir WHERE id=:dir_id;

-- name: restore_book_record_with_parent_update!
-- param: content_hash: &str
UPDATE book SET deleted_at=NULL,purged_at=NULL,trash_origin_dir_id=NULL WHERE content_hash=:content_hash;

-- name: restore_directory_record_select?
-- param: dir_id: &str
SELECT parent_id,name FROM dir WHERE id=:dir_id AND deleted_at IS NOT NULL AND purged_at IS NULL;

-- name: restore_directory_record_select_3?
-- param: trash_origin_dir_id: &str
SELECT b.content_hash,bd.dir_id,bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.trash_origin_dir_id=:trash_origin_dir_id;

-- name: restore_directory_record_update!
-- param: trash_origin_dir_id: &str
UPDATE book_dir SET deleted_at=NULL,trash_origin_dir_id=NULL WHERE trash_origin_dir_id=:trash_origin_dir_id;

-- name: restore_directory_record_update_2!
-- param: trash_origin_dir_id: &str
UPDATE book SET deleted_at=NULL,purged_at=NULL,trash_origin_dir_id=NULL WHERE trash_origin_dir_id=:trash_origin_dir_id;

-- name: transfer_book_placement_record_insert!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
-- param: is_downloaded: i32
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,deleted_at,trash_origin_dir_id) VALUES(:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,'',:is_downloaded,NULL,NULL) ON CONFLICT(dir_id,book_row_id) DO UPDATE SET file_name=excluded.file_name,is_downloaded=excluded.is_downloaded,deleted_at=NULL,trash_origin_dir_id=NULL;

-- name: transfer_book_placement_record_select?
-- param: dir_id: &str
-- param: content_hash: &str
SELECT bd.file_name,bd.is_downloaded FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id WHERE bd.dir_id=:dir_id AND book.content_hash=:content_hash AND bd.deleted_at IS NULL AND book.deleted_at IS NULL;

-- name: trash_book_record_select?
-- param: content_hash: &str
SELECT bd.dir_id,bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash=:content_hash AND bd.deleted_at IS NULL AND b.deleted_at IS NULL;

-- name: trash_book_record_update!
-- param: content_hash: &str
-- param: deleted_at: i64
UPDATE book SET deleted_at=:deleted_at,purged_at=NULL,trash_origin_dir_id=NULL WHERE content_hash=:content_hash AND deleted_at IS NULL;

-- name: trash_book_record_update_2!
-- param: content_hash: &str
-- param: deleted_at: i64
UPDATE book_dir SET deleted_at=:deleted_at,is_downloaded=0 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash) AND deleted_at IS NULL;

-- name: trash_directory_record_update!
-- param: content_hash: &str
-- param: deleted_at: i64
-- param: trash_origin_dir_id: &str
UPDATE book SET deleted_at=:deleted_at,purged_at=NULL,trash_origin_dir_id=:trash_origin_dir_id WHERE content_hash=:content_hash;

-- name: trash_directory_record_update_2!
-- param: dir_id: &str
-- param: deleted_at: i64
UPDATE dir SET deleted_at=:deleted_at,purged_at=NULL,intent_lifecycle=1 WHERE id=:dir_id;

-- name: trash_directory_record_with?
-- param: dir_id: &str
WITH RECURSIVE subtree(id) AS (SELECT id FROM dir WHERE id=:dir_id UNION ALL SELECT d.id FROM dir d JOIN subtree s ON d.parent_id=s.id WHERE d.id!=d.parent_id) SELECT b.content_hash,bd.dir_id,bd.file_name FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE bd.dir_id IN (SELECT id FROM subtree) AND bd.deleted_at IS NULL AND b.deleted_at IS NULL;

-- name: trash_directory_record_with_2?
-- param: dir_id: &str
WITH RECURSIVE subtree(id) AS (SELECT id FROM dir WHERE id=:dir_id UNION ALL SELECT child.id FROM dir child JOIN subtree ON child.parent_id=subtree.id WHERE child.id!=child.parent_id) SELECT DISTINCT book.content_hash FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id WHERE bd.dir_id IN (SELECT id FROM subtree) AND bd.deleted_at IS NULL AND book.deleted_at IS NULL AND NOT EXISTS(SELECT 1 FROM book_dir outside WHERE outside.book_row_id=book.row_id AND outside.deleted_at IS NULL AND outside.dir_id NOT IN (SELECT id FROM subtree));

-- name: trash_directory_record_with_3!
-- param: dir_id: &str
-- param: deleted_at: i64
WITH RECURSIVE subtree(id) AS (SELECT id FROM dir WHERE id=:dir_id UNION ALL SELECT child.id FROM dir child JOIN subtree ON child.parent_id=subtree.id WHERE child.id!=child.parent_id) UPDATE book_dir SET deleted_at=:deleted_at,is_downloaded=0,trash_origin_dir_id=:dir_id WHERE dir_id IN (SELECT id FROM subtree) AND deleted_at IS NULL;

-- name: schema_placements &
-- Directory tree, the root directory, and book placement within it.
create table if not exists dir(
    id              TEXT primary key,
    parent_id       TEXT not null references dir(id) on delete cascade,
    name            TEXT not null,
    name_key        TEXT generated always as (portable_name_key(name)) stored,
    -- Authoritative synchronized register. The parent/name/deleted_at columns
    -- above are the derived, filesystem-safe projection and may temporarily
    -- suppress an otherwise-present intent to break cycles/path collisions.
    intent_parent_id TEXT,
    intent_name      TEXT,
    -- 0 = present, 1 = Trash, 2 = permanently purged tombstone.
    intent_lifecycle INTEGER,
    deleted_at      INTEGER,
    purged_at       INTEGER,
    CHECK (purged_at IS NULL OR deleted_at IS NOT NULL),
    CHECK (id != '00000000-0000-0000-0000-000000000000' OR (deleted_at IS NULL AND purged_at IS NULL))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_dir_live_path
ON dir(parent_id, name_key) WHERE deleted_at IS NULL AND id != '00000000-0000-0000-0000-000000000000';

-- Recursive folder traversal starts from parent_id but cannot use
-- idx_dir_live_path when SQLite cannot prove its extra root exclusion.
CREATE INDEX IF NOT EXISTS idx_dir_live_parent
ON dir(parent_id) WHERE deleted_at IS NULL;

insert into dir(id, parent_id, name, intent_parent_id, intent_name, intent_lifecycle, deleted_at, purged_at)
values ('00000000-0000-0000-0000-000000000000', '00000000-0000-0000-0000-000000000000', '', '00000000-0000-0000-0000-000000000000', '', 0, NULL, NULL)
on conflict(id) do update set parent_id = excluded.parent_id, name = excluded.name, deleted_at = NULL, purged_at = NULL;

create table if not exists book_dir(
    dir_id          TEXT not null references dir(id) on delete cascade,
    book_row_id         INTEGER not null references book(row_id) on delete cascade,
    file_name       TEXT not null,
    -- Per-machine filesystem fingerprint (nanosecond mtime/size/inode/dev). Cheap to
    -- compute on every scan so unchanged files can be skipped without re-reading
    -- their contents (see get_book_dir_by_hash). Not unique and not portable
    -- across devices, so it is an attribute, not the key.
    local_hash      TEXT not null,
    is_downloaded   INTEGER not null default 0,
    -- Monotonic scan generation in which this placement was last seen on disk.
    -- After a full scan, locally-present rows (is_downloaded = 1) not stamped
    -- with the current generation are reconciled as vanished. Remote placements
    -- never downloaded keep 0 and are excluded via the is_downloaded filter.
    last_scan       INTEGER not null default 0,
    deleted_at      INTEGER,
    trash_origin_dir_id TEXT,
    -- A row is a file *placement*: one book in one directory.
    -- Same-path collisions remain local filesystem invariants enforced by
    -- idx_book_dir_live_path.
    PRIMARY KEY     (dir_id, book_row_id)
);

-- local_hash lost its implicit PK index when it stopped being the key; the
-- scan-skip lookup queries by it on every file, so index it explicitly.
CREATE INDEX IF NOT EXISTS idx_book_dir_local_hash ON book_dir(local_hash);

-- The composite primary key starts with dir_id and cannot efficiently answer
-- the common "all placements for this book" lookup by itself.
CREATE INDEX IF NOT EXISTS idx_book_dir_book
ON book_dir(book_row_id, deleted_at, is_downloaded);

CREATE UNIQUE INDEX IF NOT EXISTS idx_book_dir_live_path
ON book_dir(dir_id, file_name) WHERE deleted_at IS NULL;


CREATE INDEX IF NOT EXISTS idx_book_dir_live_portable_name ON book_dir(dir_id,portable_name_key(file_name)) WHERE deleted_at IS NULL;

-- name: placement_expects_hash?
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
SELECT EXISTS(SELECT 1 FROM book_dir JOIN book ON book.row_id = book_dir.book_row_id WHERE book_dir.dir_id = :dir_id AND book.content_hash = :content_hash AND book_dir.file_name = :file_name AND book_dir.deleted_at IS NULL);

-- name: dir_entry_by_path?
-- param: rel_path: &str
WITH RECURSIVE dir_paths(id, parent_id, name, path) AS (SELECT id, parent_id, name, '/' AS path FROM dir WHERE id = '00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL UNION ALL SELECT d.id, d.parent_id, d.name, CASE WHEN dir_paths.path = '/' THEN '/' || d.name ELSE dir_paths.path || '/' || d.name END FROM dir d JOIN dir_paths ON d.parent_id = dir_paths.id AND d.id != dir_paths.id WHERE d.deleted_at IS NULL) SELECT book.content_hash, bd.dir_id, bd.file_name FROM book_dir bd JOIN book ON book.row_id = bd.book_row_id JOIN dir_paths ON bd.dir_id = dir_paths.id WHERE CASE WHEN dir_paths.path = '/' THEN '/' || bd.file_name ELSE dir_paths.path || '/' || bd.file_name END = :rel_path AND bd.deleted_at IS NULL LIMIT 1;

-- name: add_book_placement!
-- param: dir_id: &str
-- param: content_hash: &str
-- param: file_name: &str
-- param: local_hash: &str
-- param: downloaded: bool
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(:dir_id,(SELECT row_id FROM book WHERE content_hash=:content_hash),:file_name,:local_hash,:downloaded);

-- name: seed_book!
-- param: content_hash: &str
-- param: title: Option<&str>
-- param: added_at: i64
-- param: format: &str
INSERT INTO book(content_hash,title,added_at,format) VALUES(:content_hash,:title,:added_at,:format);

-- name: seed_dir!
-- param: dir_id: &str
-- param: parent_id: &str
-- param: name: &str
INSERT INTO dir(id,parent_id,name) VALUES(:dir_id,:parent_id,:name);

-- name: seed_shelf_dir!
-- param: dir_id: &str
INSERT INTO dir(id,parent_id,name,intent_parent_id,intent_name,intent_lifecycle) VALUES(:dir_id,'00000000-0000-0000-0000-000000000000','Shelf','00000000-0000-0000-0000-000000000000','Shelf',0);

-- name: mark_all_placements_deleted!
UPDATE book_dir SET deleted_at=1;

-- name: mark_all_books_deleted_purged!
UPDATE book SET deleted_at=1,purged_at=1;

-- name: rename_dir!
-- param: name: &str
-- param: dir_id: &str
UPDATE dir SET name=:name WHERE id=:dir_id;

-- name: touch_all_placement_hashes!
UPDATE book_dir SET local_hash='changed-during-upload';

-- name: stale_all_placements!
UPDATE book_dir SET deleted_at=1, is_downloaded=0;

-- name: dir_path_string?
-- param: dir_id: &str
SELECT path FROM dir_paths WHERE id=:dir_id;

-- name: book_title?
-- param: content_hash: &str
SELECT title FROM book WHERE content_hash=:content_hash;

-- name: book_count?
-- param: content_hash: &str
SELECT count(*) FROM book WHERE content_hash=:content_hash;

-- name: book_count_with_title?
-- param: title: &str
SELECT count(*) FROM book WHERE title=:title;

-- name: downloaded_placement_total?
SELECT SUM(is_downloaded) FROM book_dir;

-- name: dir_name?
-- param: dir_id: &str
SELECT name FROM dir WHERE id=:dir_id;
