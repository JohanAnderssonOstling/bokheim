//! Directory and physical book-placement persistence.

use super::assets::{dir_path_from_components, get_dir_path_components};
use super::*;

pub(crate) fn directory_relative_path(conn: &Connection, dir_id: &DirId) -> Result<Option<RelativeDirPath>, rusqlite::Error> {
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

pub(crate) struct CreatedDirectory {
    pub(crate) directory: Dir,
    pub(crate) relative_path: RelativeDirPath,
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

pub(crate) fn create_local_directory(conn: &mut Connection, library_root: &Path, parent_id: &DirId, name: &FileName) -> Result<CreatedDirectory, Box<dyn std::error::Error>> {
    let tx = conn.transaction()?;
    let mut parent_is_live = false;
    tx.live_directory_exists(&parent_id.to_string(), |row| {
        parent_is_live = row.get(0)?;
        Ok(())
    })?;
    if !parent_is_live {
        return Err(format!("parent directory does not exist: {parent_id}").into());
    }
    let dir_id = DirId::new_v4();
    upsert_dir(&tx, &dir_id, parent_id, name)?;
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
    Ok(CreatedDirectory { directory: Dir::new(dir_id, name.clone(), 0, 0), relative_path })
}

#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
#[derive(Clone)]
pub struct BookDir {
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
        result.entry(fingerprint).or_insert(BookDir { dir_id, file_name, description_missing: row.get(5)? });
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
