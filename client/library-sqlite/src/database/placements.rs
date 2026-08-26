//! Directory and physical book-placement persistence.

use super::assets::{dir_path_from_components, get_dir_path_components};
use super::*;
use rusqlite::OptionalExtension;

pub fn directory_relative_path(conn: &Connection, dir_id: &DirId) -> Result<Option<RelativeDirPath>, rusqlite::Error> {
    let components = get_dir_path_components(conn, &dir_id.to_string())?;
    dir_path_from_components(&components)
}

pub fn set_book_downloaded(conn: &Connection, rel_path: &RelativeBookPath, content_hash: ContentHash, downloaded: bool) -> Result<(), rusqlite::Error> {
    let Some((found_content_hash, dir_id, _file_name)) = get_book_dir_entry_by_path(conn, rel_path)? else {
        return Ok(());
    };
    if found_content_hash != content_hash {
        return Err(invalid_text_column(0, "book placement identity mismatch"));
    }
    conn.set_book_downloaded(&dir_id.to_string(), content_hash.as_str(), i32::from(downloaded))?;
    Ok(())
}

pub fn upsert_dir(transaction: &Transaction, dir_uuid: &DirId, parent_uuid: &DirId, name: &FileName) -> Result<(), rusqlite::Error> {
    if *dir_uuid == sync_common::ROOT_DIR_ID {
        return Err(invalid_text_column(0, "the root directory is reserved"));
    }
    if dir_uuid == parent_uuid {
        return Err(invalid_text_column(0, "a directory cannot be its own parent"));
    }
    let mut closes_cycle = false;
    transaction.directory_move_closes_cycle(&parent_uuid.to_string(), &dir_uuid.to_string(), |row| {
        closes_cycle = row.get(0)?;
        Ok(())
    })?;
    if closes_cycle {
        return Err(invalid_text_column(0, "directory move would create a cycle"));
    }
    transaction.upsert_dir(&dir_uuid.to_string(), &parent_uuid.to_string(), name.as_str())?;
    Ok(())
}

pub struct CreatedDirectory {
    pub directory: Dir,
    pub relative_path: RelativeDirPath,
}

/// Creates only the synchronized directory record. Native callers compose
/// this with filesystem materialization; browser callers have no host
/// directory to create and use the same transaction directly.
pub fn create_directory_record(transaction: &Transaction, parent_id: &DirId, name: &FileName) -> Result<Dir, rusqlite::Error> {
    let mut parent_is_live = false;
    transaction.live_directory_exists(&parent_id.to_string(), |row| {
        parent_is_live = row.get(0)?;
        Ok(())
    })?;
    if !parent_is_live {
        return Err(invalid_text_column(0, format!("parent directory does not exist: {parent_id}")));
    }
    let dir_id = DirId::new_v4();
    upsert_dir(transaction, &dir_id, parent_id, name)?;
    Ok(Dir::new(dir_id, name.clone(), 0, 0))
}

pub fn folder_destinations(conn: &Connection) -> Result<Vec<LibraryFolderDestination>, rusqlite::Error> {
    let mut statement = conn.prepare(
        "WITH RECURSIVE tree(id,parent_id,label,path) AS (
             SELECT id,NULL,'Library','' FROM dir WHERE id='00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
             UNION ALL
             SELECT child.id,child.parent_id,child.name,CASE WHEN tree.path='' THEN child.name ELSE tree.path||' / '||child.name END
             FROM dir child JOIN tree ON child.parent_id=tree.id
             WHERE child.id!=child.parent_id AND child.deleted_at IS NULL
         ) SELECT id,parent_id,label,path FROM tree ORDER BY portable_name_key(path),id",
    )?;
    statement
        .query_map([], |row| {
            let id = parse_uuid_column(row.get::<_, String>(0)?, 0)?;
            let parent_id = row.get::<_, Option<String>>(1)?.map(|raw| parse_uuid_column(raw, 1)).transpose()?;
            let label = row.get(2)?;
            let path: String = row.get(3)?;
            Ok(LibraryFolderDestination { id, parent_id, label, path: if path.is_empty() { "Library".to_owned() } else { path } })
        })?
        .collect()
}

pub fn move_directory_record(transaction: &Transaction, directory_id: &DirId, requested_parent: Option<&DirId>, requested_name: Option<&str>) -> Result<String, rusqlite::Error> {
    if *directory_id == sync_common::ROOT_DIR_ID {
        return Err(invalid_text_column(0, "the library root cannot be renamed or moved"));
    }
    let (current_parent, current_name): (String, String) = transaction
        .query_row("SELECT parent_id,name FROM dir WHERE id=?1 AND deleted_at IS NULL", [directory_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?
        .ok_or_else(|| invalid_text_column(0, "folder does not exist or is in Trash"))?;
    let current_parent = parse_uuid_column(current_parent, 0)?;
    let parent_id = requested_parent.copied().unwrap_or(current_parent);
    let parent_is_descendant: bool = transaction.query_row(
        "WITH RECURSIVE subtree(id) AS (
             SELECT id FROM dir WHERE id=?1
             UNION ALL SELECT child.id FROM dir child JOIN subtree ON child.parent_id=subtree.id WHERE child.id!=child.parent_id
         ) SELECT EXISTS(SELECT 1 FROM subtree WHERE id=?2)",
        rusqlite::params![directory_id.to_string(), parent_id.to_string()],
        |row| row.get(0),
    )?;
    if parent_is_descendant {
        return Err(invalid_text_column(0, "a folder cannot be moved into itself or one of its descendants"));
    }
    let parent_exists: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM dir WHERE id=?1 AND deleted_at IS NULL)", [parent_id.to_string()], |row| row.get(0))?;
    if !parent_exists {
        return Err(invalid_text_column(0, "destination folder does not exist"));
    }
    let requested = requested_name.unwrap_or(&current_name);
    let occupied = transaction
        .prepare("SELECT name FROM dir WHERE parent_id=?1 AND id!=?2 AND deleted_at IS NULL")?
        .query_map(rusqlite::params![parent_id.to_string(), directory_id.to_string()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let chosen = library_model::unique_folder_name(requested, occupied.iter().map(String::as_str));
    if parent_id != current_parent || chosen != current_name {
        upsert_dir(transaction, directory_id, &parent_id, &chosen)?;
    }
    Ok(chosen)
}

pub fn transfer_book_placement_record(transaction: &Transaction, content_hash: &ContentHash, source_id: &DirId, destination_id: &DirId, remove_source: bool) -> Result<String, rusqlite::Error> {
    if source_id == destination_id {
        return Err(invalid_text_column(0, "source and destination folders are the same"));
    }
    let (source_name, downloaded): (String, bool) = transaction
        .query_row(
            "SELECT bd.file_name,bd.is_downloaded FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id WHERE bd.dir_id=?1 AND book.content_hash=?2 AND bd.deleted_at IS NULL AND book.deleted_at IS NULL",
            rusqlite::params![source_id.to_string(), content_hash.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| invalid_text_column(0, "book is not present in the source folder"))?;
    let destination_exists: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM dir WHERE id=?1 AND deleted_at IS NULL)", [destination_id.to_string()], |row| row.get(0))?;
    if !destination_exists {
        return Err(invalid_text_column(0, "destination folder does not exist"));
    }
    let existing: Option<String> = transaction
        .query_row("SELECT file_name FROM book_dir WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2) AND deleted_at IS NULL", rusqlite::params![destination_id.to_string(), content_hash.as_str()], |row| {
            row.get(0)
        })
        .optional()?;
    let destination_name = if let Some(existing) = existing {
        existing
    } else {
        let occupied = transaction.prepare("SELECT file_name FROM book_dir WHERE dir_id=?1 AND deleted_at IS NULL")?.query_map([destination_id.to_string()], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        let chosen = library_model::unique_file_name(&source_name, occupied.iter().map(String::as_str));
        transaction.execute(
            "INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,deleted_at,trash_origin_dir_id) VALUES(?1,(SELECT row_id FROM book WHERE content_hash=?2),?3,'',?4,NULL,NULL) ON CONFLICT(dir_id,book_row_id) DO UPDATE SET file_name=excluded.file_name,is_downloaded=excluded.is_downloaded,deleted_at=NULL,trash_origin_dir_id=NULL",
            rusqlite::params![destination_id.to_string(), content_hash.as_str(), chosen, i32::from(downloaded)],
        )?;
        chosen
    };
    if remove_source {
        transaction.execute(
            "UPDATE book_dir SET deleted_at=?3,is_downloaded=0,trash_origin_dir_id=NULL WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2) AND deleted_at IS NULL",
            rusqlite::params![source_id.to_string(), content_hash.as_str(), unix_millis()?],
        )?;
    }
    Ok(destination_name)
}

pub fn remove_book_placement_record(transaction: &Transaction, content_hash: &ContentHash, source_id: &DirId) -> Result<bool, rusqlite::Error> {
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id WHERE bd.dir_id=?1 AND book.content_hash=?2 AND bd.deleted_at IS NULL AND book.deleted_at IS NULL)",
        rusqlite::params![source_id.to_string(), content_hash.as_str()],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(invalid_text_column(0, "book is not present in this folder"));
    }
    let other_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND dir_id!=?2 AND deleted_at IS NULL",
        rusqlite::params![content_hash.as_str(), source_id.to_string()],
        |row| row.get(0),
    )?;
    let orphaned = other_count == 0;
    let now = unix_millis()?;
    transaction.execute(
        "UPDATE book_dir SET deleted_at=?3,is_downloaded=0,trash_origin_dir_id=NULL WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2) AND deleted_at IS NULL",
        rusqlite::params![source_id.to_string(), content_hash.as_str(), now],
    )?;
    if orphaned {
        transaction.execute("UPDATE book SET deleted_at=?2,purged_at=NULL,trash_origin_dir_id=NULL WHERE content_hash=?1", rusqlite::params![content_hash.as_str(), now])?;
    }
    Ok(orphaned)
}

pub fn library_trash(conn: &Connection) -> Result<LibraryTrashView, rusqlite::Error> {
    let mut book_statement = conn.prepare(
        "SELECT content_hash,COALESCE(NULLIF(title,''),content_hash),format,deleted_at
         FROM book WHERE deleted_at IS NOT NULL AND purged_at IS NULL
         ORDER BY deleted_at DESC,portable_name_key(COALESCE(title,content_hash)),content_hash",
    )?;
    let books = book_statement.query_map([], |row| Ok(TrashedBook { content_hash: ContentHash::new(&row.get::<_, String>(0)?), title: row.get(1)?, format: row.get(2)?, deleted_at: row.get(3)? }))?.collect::<Result<Vec<_>, _>>()?;
    let mut folder_statement = conn.prepare("SELECT id,name,deleted_at FROM dir WHERE deleted_at IS NOT NULL AND purged_at IS NULL AND intent_lifecycle=1 ORDER BY deleted_at DESC,name_key,id")?;
    let folders = folder_statement.query_map([], |row| Ok(TrashedFolder { id: parse_uuid_column(row.get::<_, String>(0)?, 0)?, name: row.get(1)?, deleted_at: row.get(2)? }))?.collect::<Result<Vec<_>, _>>()?;
    Ok(LibraryTrashView { books, folders })
}

pub fn trash_book_record(conn: &Connection, content_hash: &ContentHash) -> Result<(), rusqlite::Error> {
    let changed = conn.execute("UPDATE book SET deleted_at=?2,purged_at=NULL,trash_origin_dir_id=NULL WHERE content_hash=?1 AND deleted_at IS NULL", rusqlite::params![content_hash.as_str(), unix_millis()?])?;
    if changed == 0 {
        return Err(invalid_text_column(0, "book does not exist or is already in Trash"));
    }
    Ok(())
}

pub fn restore_book_record(conn: &Connection, content_hash: &ContentHash) -> Result<(), rusqlite::Error> {
    let changed = conn.execute("UPDATE book SET deleted_at=NULL,purged_at=NULL,trash_origin_dir_id=NULL WHERE content_hash=?1 AND deleted_at IS NOT NULL AND purged_at IS NULL", [content_hash.as_str()])?;
    if changed == 0 {
        return Err(invalid_text_column(0, "book is not in Trash"));
    }
    Ok(())
}

/// Files known to contain local data for a book before its disposable database
/// state is purged. Native clients can stage these paths and then call
/// [`purge_book_record`] in the same transaction.
pub struct BookPurgeInfo {
    pub format: String,
    pub local_paths: Vec<PathBuf>,
}

pub fn book_purge_info(tx: &Transaction, content_hash: &ContentHash) -> Result<BookPurgeInfo, rusqlite::Error> {
    let format =
        tx.query_row("SELECT format FROM book WHERE content_hash=?1 AND deleted_at IS NOT NULL AND purged_at IS NULL", [content_hash.as_str()], |row| row.get(0)).optional()?.ok_or_else(|| invalid_text_column(0, "book is not in Trash"))?;
    let local_paths = tx.prepare("SELECT path FROM blob_local_ref WHERE hash=?1 AND exists_flag=1")?.query_map([content_hash.as_str()], |row| row.get::<_, String>(0).map(PathBuf::from))?.collect::<Result<Vec<_>, _>>()?;
    Ok(BookPurgeInfo { format, local_paths })
}

/// Permanently removes the disposable state for a book that is in Trash while
/// retaining its synchronized identity and Purged lifecycle tombstone.
///
/// Filesystem-backed callers remove projected files separately; browser callers
/// similarly remove their OPFS publication after this transaction commits.
pub fn purge_book_record(tx: &Transaction, content_hash: &ContentHash) -> Result<(), rusqlite::Error> {
    let book_row_id = tx
        .query_row("SELECT row_id FROM book WHERE content_hash=?1 AND deleted_at IS NOT NULL AND purged_at IS NULL", [content_hash.as_str()], |row| row.get::<_, i64>(0))
        .optional()?
        .ok_or_else(|| invalid_text_column(0, "book is not in Trash"))?;
    let now = unix_millis()?;

    // Publish placement tombstones before removing their disposable local
    // rows. Other replicas use these before the Purged lifecycle arrives.
    tx.execute("UPDATE book_dir SET deleted_at=?2,trash_origin_dir_id=NULL WHERE book_row_id=?1 AND deleted_at IS NULL", rusqlite::params![book_row_id, now])?;
    for table in [
        "book_publisher",
        "book_identifier",
        "book_language",
        "book_contributor",
        "book_subject_assignment_evidence",
        "book_subject_assignment",
        "book_subject",
        "book_external_entity",
        "external_metadata_attempt",
        "book_collection",
        "book_publication_date",
        "book_publication_value",
        "catalog_publication_book",
        "annotation",
        "bookmark",
    ] {
        tx.execute(&format!("DELETE FROM {table} WHERE book_row_id=?1"), [book_row_id])?;
    }
    tx.execute("DELETE FROM book_link WHERE source_book_row_id=?1 OR target_book_row_id=?1", [book_row_id])?;
    tx.execute("DELETE FROM book_dir WHERE book_row_id=?1", [book_row_id])?;
    tx.execute("DELETE FROM audiobook_metadata WHERE content_hash=?1", [content_hash.as_str()])?;
    tx.execute("DELETE FROM blob_local_ref WHERE hash=?1", [content_hash.as_str()])?;
    tx.execute("DELETE FROM download_requests WHERE target_key=?1", [content_hash.as_str()])?;
    tx.execute("DELETE FROM remote_asset WHERE hash=?1", [content_hash.as_str()])?;
    tx.execute(
        "UPDATE book SET title=NULL,description='',description_scanned=0,publication_metadata=X'',format='',hidden_at=NULL,read_pos=NULL,read_progress=0,added_at=NULL,deleted_at=?2,purged_at=?2,trash_origin_dir_id=NULL WHERE row_id=?1",
        rusqlite::params![book_row_id, now],
    )?;
    Ok(())
}

pub fn trash_directory_record(tx: &Transaction, directory_id: &DirId) -> Result<(), rusqlite::Error> {
    if *directory_id == sync_common::ROOT_DIR_ID {
        return Err(invalid_text_column(0, "the library root cannot be moved to Trash"));
    }
    let present: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM dir WHERE id=?1 AND deleted_at IS NULL)", [directory_id.to_string()], |row| row.get(0))?;
    if !present {
        return Err(invalid_text_column(0, "folder does not exist or is already in Trash"));
    }
    let now = unix_millis()?;
    let hashes = tx.prepare("WITH RECURSIVE subtree(id) AS (SELECT id FROM dir WHERE id=?1 UNION ALL SELECT child.id FROM dir child JOIN subtree ON child.parent_id=subtree.id WHERE child.id!=child.parent_id) SELECT DISTINCT book.content_hash FROM book_dir bd JOIN book ON book.row_id=bd.book_row_id WHERE bd.dir_id IN (SELECT id FROM subtree) AND bd.deleted_at IS NULL AND book.deleted_at IS NULL AND NOT EXISTS(SELECT 1 FROM book_dir outside WHERE outside.book_row_id=book.row_id AND outside.deleted_at IS NULL AND outside.dir_id NOT IN (SELECT id FROM subtree))")?.query_map([directory_id.to_string()], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    tx.execute("WITH RECURSIVE subtree(id) AS (SELECT id FROM dir WHERE id=?1 UNION ALL SELECT child.id FROM dir child JOIN subtree ON child.parent_id=subtree.id WHERE child.id!=child.parent_id) UPDATE book_dir SET deleted_at=?2,is_downloaded=0,trash_origin_dir_id=?1 WHERE dir_id IN (SELECT id FROM subtree) AND deleted_at IS NULL", rusqlite::params![directory_id.to_string(), now])?;
    for hash in hashes {
        tx.execute("UPDATE book SET deleted_at=?2,purged_at=NULL,trash_origin_dir_id=?3 WHERE content_hash=?1", rusqlite::params![hash, now, directory_id.to_string()])?;
    }
    tx.execute("UPDATE dir SET deleted_at=?2,purged_at=NULL,intent_lifecycle=1 WHERE id=?1", rusqlite::params![directory_id.to_string(), now])?;
    Ok(())
}

pub fn restore_directory_record(tx: &Transaction, directory_id: &DirId) -> Result<String, rusqlite::Error> {
    let (stored_parent, name): (String, String) = tx
        .query_row("SELECT parent_id,name FROM dir WHERE id=?1 AND deleted_at IS NOT NULL AND purged_at IS NULL", [directory_id.to_string()], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?
        .ok_or_else(|| invalid_text_column(0, "folder is not in Trash"))?;
    let parent = if tx.query_row("SELECT EXISTS(SELECT 1 FROM dir WHERE id=?1 AND deleted_at IS NULL)", [&stored_parent], |row| row.get::<_, bool>(0))? { parse_uuid_column(stored_parent, 0)? } else { sync_common::ROOT_DIR_ID };
    let occupied =
        tx.prepare("SELECT name FROM dir WHERE parent_id=?1 AND id!=?2 AND deleted_at IS NULL")?.query_map(rusqlite::params![parent.to_string(), directory_id.to_string()], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    let chosen = library_model::unique_folder_name(&name, occupied.iter().map(String::as_str));
    tx.execute("UPDATE dir SET parent_id=?2,name=?3,intent_parent_id=?2,intent_name=?3,deleted_at=NULL,purged_at=NULL,intent_lifecycle=0 WHERE id=?1", rusqlite::params![directory_id.to_string(), parent.to_string(), chosen])?;
    tx.execute("UPDATE book_dir SET deleted_at=NULL,trash_origin_dir_id=NULL WHERE trash_origin_dir_id=?1", [directory_id.to_string()])?;
    tx.execute("UPDATE book SET deleted_at=NULL,purged_at=NULL,trash_origin_dir_id=NULL WHERE trash_origin_dir_id=?1", [directory_id.to_string()])?;
    Ok(chosen)
}

pub fn purge_directory_record(tx: &Transaction, directory_id: &DirId) -> Result<(), rusqlite::Error> {
    let changed=tx.execute("WITH RECURSIVE subtree(id) AS (SELECT id FROM dir WHERE id=?1 AND deleted_at IS NOT NULL AND purged_at IS NULL UNION ALL SELECT child.id FROM dir child JOIN subtree ON child.parent_id=subtree.id WHERE child.id!=child.parent_id) UPDATE dir SET deleted_at=?2,purged_at=?2,intent_lifecycle=2 WHERE id IN (SELECT id FROM subtree)",rusqlite::params![directory_id.to_string(),unix_millis()?])?;
    if changed == 0 {
        return Err(invalid_text_column(0, "folder is not in Trash"));
    }
    Ok(())
}

/// Permanently purges every current Trash entry in one transaction. Folder
/// purging deliberately does not imply book purging, so both collections are
/// processed explicitly.
pub fn empty_trash_records(tx: &Transaction) -> Result<Vec<ContentHash>, rusqlite::Error> {
    let trash = library_trash(tx)?;
    for folder in trash.folders {
        purge_directory_record(tx, &folder.id)?;
    }
    let content_hashes = trash.books.into_iter().map(|book| book.content_hash).collect::<Vec<_>>();
    for content_hash in &content_hashes {
        purge_book_record(tx, content_hash)?;
    }
    Ok(content_hashes)
}

fn rollback_created_directory(path: &Path, dir_id: &DirId) {
    let marker = path.join(".biblos_uuid");
    let expected = dir_id.to_string();
    if fs::read_to_string(&marker).ok().as_deref() != Some(expected.as_str()) {
        return;
    }
    if fs::remove_file(&marker).is_ok() && fs::remove_dir(path).is_err() {
        let _ = fs::write(marker, dir_id.to_string());
    }
}

pub fn create_local_directory(conn: &mut Connection, library_root: &Path, parent_id: &DirId, name: &FileName) -> Result<CreatedDirectory, Box<dyn std::error::Error>> {
    // The scanner can write as soon as a library opens. Acquire the write lock
    // before reading the parent so this transaction waits instead of failing a
    // deferred read-to-write upgrade with SQLITE_BUSY.
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let directory = create_directory_record(&tx, parent_id, name)?;
    let dir_id = *directory.id();
    let components = get_dir_path_components(&tx, &dir_id.to_string())?;
    let relative_path = dir_path_from_components(&components)?.ok_or("created directory has no projected path")?;
    materialize_dir_components_with_markers(library_root, &components, false)?;
    let absolute_path = relative_path.resolve_under(library_root);
    match fs::symlink_metadata(&absolute_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => return Err(format!("a filesystem entry already exists at {}", absolute_path.display()).into()),
        Err(error) => return Err(error.into()),
    }
    fs::create_dir(&absolute_path)?;
    let marker_path = absolute_path.join(".biblos_uuid");
    let marker_result = (|| {
        let mut marker = fs::OpenOptions::new().write(true).create_new(true).open(marker_path)?;
        marker.write_all(dir_id.to_string().as_bytes())?;
        marker.sync_all()
    })();
    if let Err(error) = marker_result {
        let _ = fs::remove_file(absolute_path.join(".biblos_uuid"));
        let _ = fs::remove_dir(&absolute_path);
        return Err(error.into());
    }
    if let Err(error) = tx.commit() {
        rollback_created_directory(&absolute_path, &dir_id);
        return Err(error.into());
    }
    Ok(CreatedDirectory { directory, relative_path })
}

#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
#[derive(Clone)]
pub struct BookDir {
    pub content_hash: ContentHash,
    pub dir_id: DirId,
    pub file_name: FileName,
    pub description_missing: bool,
}

#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub fn get_book_dirs_by_local_fingerprint(conn: &Connection) -> Result<HashMap<u64, BookDir>, rusqlite::Error> {
    let mut result = HashMap::new();
    conn.get_local_fingerprint_book_dirs(|row| {
        let Ok(fingerprint) = row.get::<_, String>(0)?.parse::<u64>() else { return Ok(()) };
        let dir_id = parse_uuid_column(row.get::<_, String>(2)?, 2)?;
        let file_name = row.get::<_, String>(3)?;
        result.entry(fingerprint).or_insert(BookDir { content_hash: ContentHash::new(&row.get::<_, String>(1)?), dir_id, file_name, description_missing: row.get(5)? });
        Ok(())
    })?;
    Ok(result)
}

#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub fn get_tombstoned_book_file_keys(conn: &Connection) -> Result<HashSet<(DirId, FileName)>, rusqlite::Error> {
    let mut statement = conn.prepare_cached(
        "SELECT removed.dir_id, removed.file_name
         FROM book_dir removed
         WHERE removed.deleted_at IS NOT NULL
           AND NOT EXISTS (
               SELECT 1 FROM book_dir live
               WHERE live.dir_id = removed.dir_id
                 AND live.file_name = removed.file_name
                 AND live.deleted_at IS NULL
           )",
    )?;
    let rows = statement.query_map([], |row| {
        let dir_id = parse_uuid_column(row.get::<_, String>(0)?, 0)?;
        let file_name = row.get::<_, String>(1)?;
        Ok((dir_id, file_name))
    })?;
    rows.collect()
}

#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub fn insert_book(transaction: &Transaction, content_hash: ContentHash, format: BookFormat) -> Result<(), rusqlite::Error> {
    transaction.insert_scanned_book(content_hash.as_str(), unix_millis()?, format.canonical_extension())?;
    Ok(())
}

pub fn insert_local_book_dir(transaction: &Transaction, dir_id: &DirId, content_hash: ContentHash, file_name: &FileName, local_hash: u64, last_scan: i64) -> Result<(), rusqlite::Error> {
    transaction.insert_book_dir(&dir_id.to_string(), content_hash.as_str(), file_name.as_str(), &local_hash.to_string(), last_scan, 1)?;
    Ok(())
}

#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub fn mark_known_book_dirs_seen(transaction: &Transaction, scan_id: i64, placements: &[(DirId, FileName, u64)]) -> Result<(), rusqlite::Error> {
    // Keep each statement well below SQLite's conservative 999-variable limit.
    // One scan transaction contains at most SCAN_DB_BATCH_SIZE entries, so this
    // normally executes exactly one statement instead of one UPDATE per book.
    const PLACEMENTS_PER_STATEMENT: usize = 250;
    for placements in placements.chunks(PLACEMENTS_PER_STATEMENT) {
        if placements.is_empty() {
            continue;
        }
        let tuples = std::iter::repeat_n("(?, ?, ?)", placements.len()).collect::<Vec<_>>().join(", ");
        let sql = format!("UPDATE book_dir SET last_scan = ? WHERE deleted_at IS NULL AND (dir_id, file_name, local_hash) IN ({tuples})");
        let mut values = Vec::with_capacity(1 + placements.len() * 3);
        values.push(rusqlite::types::Value::Integer(scan_id));
        for (dir_id, file_name, local_hash) in placements {
            values.push(rusqlite::types::Value::Text(dir_id.to_string()));
            values.push(rusqlite::types::Value::Text(file_name.clone()));
            values.push(rusqlite::types::Value::Text(local_hash.to_string()));
        }
        transaction.execute(&sql, rusqlite::params_from_iter(values))?;
    }
    Ok(())
}

#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub fn book_has_live_placement(transaction: &Transaction, dir_id: &DirId, content_hash: ContentHash) -> Result<bool, rusqlite::Error> {
    let mut exists = false;
    transaction.book_has_live_placement(&dir_id.to_string(), content_hash.as_str(), |row| {
        exists = row.get(0)?;
        Ok(())
    })?;
    Ok(exists)
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::fixture_content_hash;

    #[test]
    fn observed_placements_are_marked_in_bulk_and_tombstones_are_preloaded() {
        let mut connection = Connection::open_in_memory().unwrap();
        super::super::configure_connection(&connection).unwrap();
        super::super::init_db(&connection).unwrap();
        let root = sync_common::ROOT_DIR_ID;
        let first = fixture_content_hash(1);
        let second = fixture_content_hash(2);
        let removed = fixture_content_hash(3);
        {
            let transaction = connection.transaction().unwrap();
            insert_book(&transaction, first.clone(), BookFormat::Epub).unwrap();
            insert_book(&transaction, second.clone(), BookFormat::Epub).unwrap();
            insert_book(&transaction, removed.clone(), BookFormat::Epub).unwrap();
            insert_local_book_dir(&transaction, &root, first, &"first.epub".to_owned(), 11, 1).unwrap();
            insert_local_book_dir(&transaction, &root, second, &"second.epub".to_owned(), 22, 1).unwrap();
            insert_local_book_dir(&transaction, &root, removed, &"removed.epub".to_owned(), 33, 1).unwrap();
            transaction.execute("UPDATE book_dir SET deleted_at=1 WHERE file_name='removed.epub'", []).unwrap();
            transaction.commit().unwrap();
        }

        let transaction = connection.transaction().unwrap();
        mark_known_book_dirs_seen(&transaction, 9, &[(root, "first.epub".to_owned(), 11), (root, "second.epub".to_owned(), 22)]).unwrap();
        transaction.commit().unwrap();

        let scans = connection.prepare("SELECT file_name, last_scan FROM book_dir ORDER BY file_name").unwrap().query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(scans, vec![("first.epub".to_owned(), 9), ("removed.epub".to_owned(), 1), ("second.epub".to_owned(), 9)]);
        assert_eq!(get_tombstoned_book_file_keys(&connection).unwrap(), HashSet::from([(root, "removed.epub".to_owned())]));
    }
}
