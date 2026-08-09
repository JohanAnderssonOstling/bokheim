//! Opens one library and constructs the resources owned by its handle.

use super::LibraryBackend;
use crate::app::{AppBackend, BackendContext};
use crate::library::database as db;
use crate::platform::scanner::LibraryScanner;
use crate::sync::SyncManager;
use crate::LibraryRoot;
use crate::{ServerUrl, ServerUrlError};
use crossbeam_channel::{unbounded, Receiver};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use sync_common::LibraryId;

fn sync_server_url() -> Result<ServerUrl, ServerUrlError> {
    Ok(crate::api_server_url())
}

impl LibraryBackend {
    pub(crate) fn open(app: &AppBackend, context: &BackendContext, library_path: &str, library_id: &str) -> Result<(Self, Receiver<crate::platform::scanner::LibraryEvent>), Box<dyn std::error::Error>> {
        Self::open_with_server_url(app, context, library_path, library_id, sync_server_url()?)
    }

    #[cfg(test)]
    pub(crate) fn open_for_test(app: &AppBackend, context: &BackendContext, library_path: &str, library_id: &str, server_url: ServerUrl) -> Result<(Self, Receiver<crate::platform::scanner::LibraryEvent>), Box<dyn std::error::Error>> {
        Self::open_with_server_url(app, context, library_path, library_id, server_url)
    }

    fn open_with_server_url(app: &AppBackend, context: &BackendContext, library_path: &str, library_id: &str, server_url: ServerUrl) -> Result<(Self, Receiver<crate::platform::scanner::LibraryEvent>), Box<dyn std::error::Error>> {
        let library_root = LibraryRoot::open(Path::new(library_path))?;
        let library_id = LibraryId::parse(library_id)?;
        let metadata_dir = library_root.as_path().join(crate::APP_HIDDEN_DIR);
        fs::create_dir_all(metadata_dir.join("thumbnails"))?;
        let db_path = metadata_dir.join("library.db");

        // A stale-but-recognized schema version is migrated in place by
        // `db::init_db` (see `migrate_legacy_contributor_schema`), preserving
        // local-only user data (annotations, bookmarks, book links). Only a
        // database whose `user_version` isn't a valid PRAGMA integer at all
        // (corrupt or foreign file) is wiped and rebuilt from scratch.
        if db_path.exists() {
            let readable = rusqlite::Connection::open(&db_path).and_then(|conn| conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))).is_ok();
            if !readable {
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

        let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
            storage_sqlite::configure_connection(conn, std::time::Duration::from_secs(10))?;
            db::configure_sync_connection(conn)
        });
        let pool = Pool::builder().max_size(20).connection_timeout(std::time::Duration::from_secs(30)).build(manager)?;
        let client_id = {
            let conn = pool.get()?;
            db::init_db(&conn)?
        };
        let (event_tx, event_rx) = unbounded();
        let scanner = LibraryScanner::from_root(context.clone(), library_root.clone(), pool.clone(), event_tx.clone())?;
        let sync_manager = Arc::new(SyncManager::new(pool.clone(), client_id, server_url, library_id.clone(), library_root.clone(), context.app_data_dir().join("session.json"), Some(event_tx.clone()), context.transfer_runtime())?);
        let library = Self::from_parts(app.downgrade(), context.clone(), library_id, library_root, pool, sync_manager, event_tx, scanner);
        Ok((library, event_rx))
    }
}
