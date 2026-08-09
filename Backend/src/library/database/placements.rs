//! Directory and physical book-placement persistence.

use super::assets::{dir_path_from_components, get_dir_path_components};
use super::*;

pub fn set_book_downloaded(conn: &Connection, rel_path: &RelativeBookPath, content_hash: ContentHash, downloaded: bool) -> Result<(), rusqlite::Error> {
    let Some((found_content_hash, dir_id, _file_name)) = get_book_dir_entry_by_path(conn, rel_path)? else {
        return Ok(());
    };
    if found_content_hash != content_hash {
        return Err(invalid_text_column(0, "book placement identity mismatch"));
    }
    conn.set_book_downloaded(dir_id.as_str(), content_hash.as_str(), i32::from(downloaded))?;
    Ok(())
}

pub fn upsert_dir(transaction: &Transaction, dir_uuid: &DirId, parent_uuid: &DirId, name: &FileName) -> Result<(), rusqlite::Error> {
    if dir_uuid.as_str() == "__root__" {
        return Err(invalid_text_column(0, "the root directory is reserved"));
    }
    if dir_uuid == parent_uuid {
        return Err(invalid_text_column(0, "a directory cannot be its own parent"));
    }
    let mut closes_cycle = false;
    transaction.directory_move_closes_cycle(parent_uuid.as_str(), dir_uuid.as_str(), |row| {
        closes_cycle = row.get(0)?;
        Ok(())
    })?;
    if closes_cycle {
        return Err(invalid_text_column(0, "directory move would create a cycle"));
    }
    transaction.upsert_dir(dir_uuid.as_str(), parent_uuid.as_str(), name.as_str())?;
    Ok(())
}

pub(crate) struct CreatedDirectory {
    pub(crate) directory: Dir,
    pub(crate) relative_path: RelativeDirPath,
}

fn rollback_created_directory(path: &Path, dir_id: &DirId) {
    let marker = path.join(".biblos_uuid");
    if fs::read_to_string(&marker).ok().as_deref() != Some(dir_id.as_str()) {
        return;
    }
    if fs::remove_file(&marker).is_ok() && fs::remove_dir(path).is_err() {
        let _ = fs::write(marker, dir_id.as_str());
    }
}

pub(crate) fn create_local_directory(conn: &mut Connection, library_root: &LibraryRoot, parent_id: &DirId, name: &FileName) -> Result<CreatedDirectory, Box<dyn std::error::Error>> {
    let tx = conn.transaction()?;
    let mut parent_is_live = false;
    tx.live_directory_exists(parent_id.as_str(), |row| {
        parent_is_live = row.get(0)?;
        Ok(())
    })?;
    if !parent_is_live {
        return Err(format!("parent directory does not exist: {parent_id}").into());
    }
    let dir_id = DirId::parse(&Uuid::new_v4().to_string())?;
    upsert_dir(&tx, &dir_id, parent_id, name)?;
    let components = get_dir_path_components(&tx, dir_id.as_str())?;
    let relative_path = dir_path_from_components(&components)?.ok_or("created directory has no projected path")?;
    materialize_dir_components_with_markers(library_root, &components, false)?;
    let absolute_path = library_root.resolve_dir(&relative_path);
    match fs::symlink_metadata(&absolute_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => return Err(format!("a filesystem entry already exists at {}", absolute_path.display()).into()),
        Err(error) => return Err(error.into()),
    }
    fs::create_dir(&absolute_path)?;
    let marker_path = absolute_path.join(".biblos_uuid");
    let marker_result = (|| {
        let mut marker = fs::OpenOptions::new().write(true).create_new(true).open(marker_path)?;
        marker.write_all(dir_id.as_str().as_bytes())?;
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
        let dir_id = DirId::parse(&row.get::<_, String>(2)?).map_err(|error| invalid_text_column(2, error.to_string()))?;
        let file_name = FileName::parse(&row.get::<_, String>(3)?).map_err(|error| invalid_text_column(3, error.to_string()))?;
        result.entry(fingerprint).or_insert(BookDir { dir_id, file_name, description_missing: row.get(5)? });
        Ok(())
    })?;
    Ok(result)
}

#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub fn insert_book(transaction: &Transaction, content_hash: ContentHash, format: BookFormat) -> Result<(), rusqlite::Error> {
    transaction.insert_scanned_book(content_hash.as_str(), unix_millis()?, format.canonical_extension())?;
    Ok(())
}

pub fn insert_local_book_dir(transaction: &Transaction, dir_id: &DirId, content_hash: ContentHash, file_name: &FileName, local_hash: u64, last_scan: i64) -> Result<(), rusqlite::Error> {
    transaction.insert_book_dir(dir_id.as_str(), content_hash.as_str(), file_name.as_str(), &local_hash.to_string(), last_scan, 1)?;
    Ok(())
}

#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub fn mark_known_book_dir_seen(transaction: &Transaction, scan_id: i64, dir_id: &DirId, file_name: &FileName, local_hash: u64) -> Result<(), rusqlite::Error> {
    transaction.mark_known_book_dir_seen(scan_id, dir_id.as_str(), file_name.as_str(), &local_hash.to_string()).map(|_| ())
}

#[cfg(any(test, all(feature = "scanner", not(target_os = "android"))))]
pub fn book_has_live_placement(transaction: &Transaction, dir_id: &DirId, content_hash: ContentHash) -> Result<bool, rusqlite::Error> {
    let mut exists = false;
    transaction.book_has_live_placement(dir_id.as_str(), content_hash.as_str(), |row| {
        exists = row.get(0)?;
        Ok(())
    })?;
    Ok(exists)
}
