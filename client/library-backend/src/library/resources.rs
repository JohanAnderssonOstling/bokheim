//! Resource construction for one concrete library session.
//!
//! The application registry decides *which* location and account policy to
//! use.  Everything opened from that location belongs to the library session.

use std::path::{Path, PathBuf};
use crate::asset_store::AssetStore;
use crate::library::discovery::LibraryScanner;
use crate::notification_interest::NotificationInterest;
use crate::sync::{LibrarySync, LibrarySyncConfig};
use crate::BackendError;
use library_database::Database;
use std::sync::Arc;

use super::LibrarySession;

pub struct LibraryRuntimeSpec {
    pub config: super::open::LibraryOpenConfig,
    pub account: crate::integration::LibraryAccount,
    pub notification_interest: NotificationInterest,
    pub remote_changes: crate::notification_interest::RemoteChanges,
}

impl LibrarySession {
    pub(crate) fn open(spec: LibraryRuntimeSpec) -> Result<Self, BackendError> {
        let default_sync_server_url = spec.config.default_sync_server_url.clone();
        let metadata_server_url = spec.config.metadata_server_url.clone();
        // One connection per library file, owned by the session. Sync and
        // asset workers borrow it through the owner; their futures stay
        // local to their tasks, so no sharing primitive is needed.
        let database = Arc::new(Database::open(&spec.config.database_locator)?);
        let cpu = {
            cpu_host::HOST
                .get_or_init(|| -> std::sync::Arc<dyn cpu_host::CpuHost> {
                    #[cfg(target_arch = "wasm32")]
                    {
                        std::sync::Arc::new(client_platform_web::cpu_host::WebHost::default())
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        std::sync::Arc::new(client_platform_native::cpu_host::NativeHost::default())
                    }
                })
                .clone()
        };
        let assets = open_assets(&spec.config.root, &spec.config.database_locator, &cpu)?;
        let transfer_history_path = history_path(&spec.config.root, &spec.config.database_locator);
        // `Database::open` already configured the connection (pragmas, sync
        // functions) internally.
        let replica_id = database.initialize_library()?;
        database.set_remote_library_name(&spec.config.display_name)?;
        let (event_tx, event_rx) = async_channel::unbounded();
        let scanner = {
            use crate::library::discovery::*;
            let root: &str = &spec.config.root;

            LibraryScanner { root: root.to_owned() }
        };
        database.set_asset_storage_policy(spec.config.asset_storage_enabled, spec.config.reset_cloud_presence)?;
        let sync = Arc::new(LibrarySync::new(LibrarySyncConfig {
            database_path: spec.config.database_locator.clone().into(),
            account: Some(spec.account.clone()),
            transfer_queue: Arc::new(library_runtime::transfer_history::queue_with_history(&database, transfer_history_path)),
            assets: assets.clone(),
            replica_id,
            server_url: spec.account.current().map_err(BackendError::operation)?.map(|session| session.server_url().clone()).unwrap_or(default_sync_server_url),
            library_id: spec.config.id,
            event_tx: Some(event_tx.clone()),
            credentials: spec.account.credentials(),
            notification_interest: spec.notification_interest,
            cpu: cpu.clone(),
        }));
        sync.set_asset_storage_enabled(spec.config.asset_storage_enabled);
        let metadata = metadata_client::MetadataClient::new(&metadata_server_url).map_err(BackendError::operation)?;
        let (sync_wake, sync_requests) = library_runtime::scheduler::SyncScheduler::new();
        let (asset_wake, asset_requests) = library_runtime::scheduler::SyncScheduler::new();
        let library = Self {
            id: spec.config.id,
            db: database,
            assets,
            metadata,
            sync,
            event_tx,
            event_rx: Some(event_rx),
            subscriptions: Default::default(),
            runtime: Arc::new(crate::library::session::LibraryRuntime::default()),
            workers: super::workers::LibraryWorkers::default(),
            sync_wake,
            sync_requests: Some(sync_requests),
            asset_wake,
            asset_requests: Some(asset_requests),
            account: spec.account,
            remote_changes: spec.remote_changes,
            _scanner: Arc::new(scanner),
            cpu,
            filesystem_scanning: std::cell::Cell::new(false),
            file_work_error: Default::default(),
        };
        // The initial scan (if needed) is triggered by the caller after the
        // actor spawns: the scanner now does async inspection work, and this
        // constructor runs synchronously on the library's blocking-open path,
        // before that actor (and its async command channel) exists.
        Ok(library)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn open_assets(root: &str, _database_locator: &str, _cpu: &std::sync::Arc<dyn cpu_host::CpuHost>) -> Result<AssetStore, BackendError> {
    crate::asset_store::open(root).map_err(BackendError::operation)
}

#[cfg(target_arch = "wasm32")]
fn open_assets(root: &str, _database_locator: &str, cpu: &std::sync::Arc<dyn cpu_host::CpuHost>) -> Result<AssetStore, BackendError> {
    Ok(crate::asset_store::open(root, cpu.clone()).map_err(BackendError::operation)?)
}

#[cfg(not(target_arch = "wasm32"))]
fn history_path(_root: &str, database_locator: &str) -> PathBuf {
    Path::new(database_locator).parent().expect("library database locator has a parent").join("transfer-history-v1.msgpack")
}

#[cfg(target_arch = "wasm32")]
fn history_path(root: &str, _database_locator: &str) -> PathBuf {
    Path::new(root).join(crate::APP_HIDDEN_DIR).join("transfer-history-v1.msgpack")
}
