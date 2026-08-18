//! Opens one library and constructs the resources owned by its handle.

use super::LibraryBackend;
use crate::app::{AppBackend, BackendContext};
use crate::library::database as db;
use crate::platform::scanner::LibraryScanner;
use crate::sync::SyncManager;
use crate::LibraryId;
use crate::{ServerUrl, ServerUrlError};
use crossbeam_channel::{unbounded, Receiver};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn sync_server_url() -> Result<ServerUrl, ServerUrlError> {
    Ok(crate::api_server_url())
}

fn canonical_library_root(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("library root is not a directory: {}", path.display())));
    }
    Ok(canonical)
}

impl LibraryBackend {
    pub(crate) fn open(app: &AppBackend, context: &BackendContext, library_path: &str, library_id: LibraryId) -> Result<(Self, Receiver<crate::platform::scanner::LibraryEvent>), Box<dyn std::error::Error>> {
        Self::open_with_server_url(app, context, library_path, library_id, sync_server_url()?, true)
    }

    pub(crate) fn open_deferred_scan(app: &AppBackend, context: &BackendContext, library_path: &str, library_id: LibraryId) -> Result<(Self, Receiver<crate::platform::scanner::LibraryEvent>), Box<dyn std::error::Error>> {
        Self::open_with_server_url(app, context, library_path, library_id, sync_server_url()?, false)
    }

    #[cfg(test)]
    pub(crate) fn open_for_test(app: &AppBackend, context: &BackendContext, library_path: &str, library_id: LibraryId, server_url: ServerUrl) -> Result<(Self, Receiver<crate::platform::scanner::LibraryEvent>), Box<dyn std::error::Error>> {
        Self::open_with_server_url(app, context, library_path, library_id, server_url, true)
    }

    fn open_with_server_url(
        app: &AppBackend, context: &BackendContext, library_path: &str, library_id: LibraryId, server_url: ServerUrl, start_scan: bool,
    ) -> Result<(Self, Receiver<crate::platform::scanner::LibraryEvent>), Box<dyn std::error::Error>> {
        let library_root = canonical_library_root(Path::new(library_path))?;
        let metadata_dir = library_root.join(crate::APP_HIDDEN_DIR);
        fs::create_dir_all(metadata_dir.join("thumbnails"))?;
        let db_path = metadata_dir.join("library.db");

        // The content-hash/row-id schema deliberately has no compatibility
        // mode. A stale, corrupt, or foreign database is rebuilt and the
        // scanner repopulates it from the library files.
        if db_path.exists() {
            let current = rusqlite::Connection::open(&db_path).and_then(|conn| conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))).is_ok_and(|version| version == db::LIBRARY_SCHEMA_VERSION);
            if !current {
                for suffix in ["", "-wal", "-shm"] {
                    let mut path = db_path.as_os_str().to_os_string();
                    path.push(suffix);
                    match fs::remove_file(PathBuf::from(path)) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(Box::new(error)),
                    }
                }
            }
        }

        let manager = SqliteConnectionManager::file(db_path).with_init(|conn| db::configure_connection(conn));
        let pool = Pool::builder().max_size(20).connection_timeout(std::time::Duration::from_secs(30)).build(manager)?;
        let replica_id = {
            let conn = pool.get()?;
            db::init_db(&conn)?
        };
        let (event_tx, event_rx) = unbounded();
        let scanner = LibraryScanner::from_root(context.clone(), library_root.clone(), pool.clone(), event_tx.clone())?;
        if start_scan {
            scanner.start_scanning();
        }
        let pending_thumbnail_jobs = scanner.pending_thumbnail_jobs();
        let sync_manager = Arc::new(SyncManager::new(
            pool.clone(),
            replica_id,
            server_url,
            library_id.clone(),
            library_root.clone(),
            context.app_data_dir().join("session.json"),
            Some(event_tx.clone()),
            context.transfer_runtime(),
            pending_thumbnail_jobs,
        )?);
        let library = Self::from_parts(app.downgrade(), context.clone(), library_id, library_root, pool, sync_manager, event_tx, scanner);
        Ok((library, event_rx))
    }
}
