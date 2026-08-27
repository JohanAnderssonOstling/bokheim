//! Opens one library and constructs the resources owned by its handle.

use super::LibraryBackend;
use crate::app::{AppBackend, BackendContext};
use crate::library::database as db;
use crate::platform::database::Database;
use crate::platform::library_events::{library_event_channel, LibraryEventReceiver};
#[cfg(all(feature = "scanner", not(target_os = "android")))]
use crate::platform::scanner::LibraryScanner;
use crate::sync::{SyncManager, SyncManagerConfig};
use crate::LibraryId;
use crate::{ServerUrl, ServerUrlError};
use std::sync::Arc;

fn sync_server_url() -> Result<ServerUrl, ServerUrlError> {
    Ok(crate::api_server_url())
}

impl LibraryBackend {
    #[cfg(all(test, feature = "scanner", not(target_os = "android")))]
    pub(crate) fn open(app: &AppBackend, context: &BackendContext, library_path: &str, library_id: LibraryId) -> Result<(Self, LibraryEventReceiver), Box<dyn std::error::Error>> {
        let (library, events) = Self::open_with_server_url(app, context, library_path, library_id, sync_server_url()?)?;
        library.start_scanner();
        Ok((library, events))
    }

    pub(crate) fn open_deferred_scan(app: &AppBackend, context: &BackendContext, library_path: &str, library_id: LibraryId) -> Result<(Self, LibraryEventReceiver), Box<dyn std::error::Error>> {
        Self::open_with_server_url(app, context, library_path, library_id, sync_server_url()?)
    }

    fn open_with_server_url(app: &AppBackend, context: &BackendContext, library_path: &str, library_id: LibraryId, server_url: ServerUrl) -> Result<(Self, LibraryEventReceiver), Box<dyn std::error::Error>> {
        let database = Database::open_library(library_path, db::configure_connection)?;
        let replica_id = {
            let conn = database.get()?;
            db::init_db(&conn)?
        };
        let (event_tx, event_rx) = library_event_channel();
        let pending_thumbnail_jobs = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        #[cfg(all(feature = "scanner", not(target_os = "android")))]
        let scanner = LibraryScanner::from_root(library_path, database.clone(), event_tx.clone(), Arc::clone(&pending_thumbnail_jobs))?;
        let sync_manager =
            Arc::new(SyncManager::new(SyncManagerConfig { database: database.clone(), replica_id, server_url, library_id, event_tx: Some(event_tx.clone()), transfer_runtime: context.transfer_runtime(), pending_thumbnail_jobs })?);
        let library = Self::from_parts(
            app.downgrade(),
            library_id,
            database,
            sync_manager,
            event_tx,
            #[cfg(all(feature = "scanner", not(target_os = "android")))]
            scanner,
        );
        Ok((library, event_rx))
    }
}
