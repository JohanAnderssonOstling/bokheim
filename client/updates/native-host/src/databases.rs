//! Database discovery and migration workspace sizing for native update hosts.
use update_client::UpdateError;
use rusqlite::{Connection, OpenFlags};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn paths(data: &Path) -> Result<Vec<PathBuf>, UpdateError> {
    let mut paths = Vec::new();
    let registry = data.join("app-state-v1.sqlite3");
    if registry.try_exists().map_err(UpdateError::operation)? {
        paths.push(registry);
    }
    let libraries = data.join("libraries");
    if libraries.try_exists().map_err(UpdateError::operation)? {
        for entry in fs::read_dir(libraries).map_err(UpdateError::operation)? {
            let entry = entry.map_err(UpdateError::operation)?;
            if !entry.file_type().map_err(UpdateError::operation)?.is_dir() {
                continue;
            }
            let path = entry.path().join("library.db");
            if path.try_exists().map_err(UpdateError::operation)? {
                paths.push(path);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

pub(crate) fn workspace(data: &Path) -> Result<u64, UpdateError> {
    let mut total = 0_u64;
    for path in paths(data)? {
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(UpdateError::operation)?;
        let pages: i64 = db.pragma_query_value(None, "page_count", |row| row.get(0)).map_err(UpdateError::operation)?;
        let size: i64 = db.pragma_query_value(None, "page_size", |row| row.get(0)).map_err(UpdateError::operation)?;
        let pages = u64::try_from(pages).map_err(UpdateError::operation)?;
        let size = u64::try_from(size).map_err(UpdateError::operation)?;
        // Backup, candidate copy and migration journal.
        total = total.checked_add(pages.checked_mul(size).and_then(|n| n.checked_mul(3)).ok_or("database workspace overflow")?).ok_or("workspace overflow")?;
    }
    Ok(total)
}
