//! Stable services, notifications, and reader resources for one open library.
//!
//! The registry resolves storage and account policy before opening resources.
//! Folder changes replace file access while keeping the other services.

use super::activity::LibraryActivity;
use super::events::{LibraryEventRouter, LibraryEventState};
#[cfg(not(target_arch = "wasm32"))]
use crate::asset_store::AssetStore;
use crate::executor::BackendExecutor;
#[cfg(not(target_arch = "wasm32"))]
use crate::DownloadState;
use crate::{BackendError, LibraryId};
use account_client::Url;
use asset_transfer::transfer::AssetExecutor;
use cpu_host::CpuHost;
use library_database::DatabaseClient;
#[cfg(not(target_arch = "wasm32"))]
use library_database::DatabaseError;
use library_enrichment::{CoverClient, LibraryEnrichment, MetadataClient};
use library_placement::LibraryState;
use library_services::account::{LibraryAccount, LibraryAuthentication};
use library_services::interest::NotificationInterest;
use library_sync::{LibrarySync, LibrarySyncConfig};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use sync_common::storage::LibraryStorage;
use tokio::sync::RwLock;

/// Storage policy and host services needed to open a library.
/// The registry resolves these values before constructing backend resources.
pub struct LibraryOpenOptions {
    pub id: LibraryId,
    pub root: String,
    pub database_locator: String,
    pub default_sync_server_url: Url,
    pub metadata_server_url: String,
    pub storage: LibraryStorage,
    pub reset_cloud_presence: bool,
    /// Discard local file observations after recovering a storage root.
    #[cfg(not(target_arch = "wasm32"))]
    pub reset_local_presence: bool,
    pub account: LibraryAccount,
    pub cpu: Arc<dyn CpuHost>,
    pub notification_interest: NotificationInterest,
    #[cfg(not(target_arch = "wasm32"))]
    pub root_failures: Option<async_channel::Sender<LibraryId>>,
}

pub(crate) struct LibraryResources {
    pub(crate) id: LibraryId,
    /// The connection stays stable when the filesystem root is rebound.
    pub(crate) database: DatabaseClient,
    pub(crate) enrichment: LibraryEnrichment,
    pub(crate) sync: Arc<LibrarySync>,
    pub(crate) transfers: AssetExecutor,
    /// Publishes events and wakes the workers that follow them.
    pub(crate) events: LibraryEventRouter,
    /// The shared owner closes subscriptions before stopping workers.
    pub(crate) event_state: Mutex<LibraryEventState>,
    #[cfg(target_arch = "wasm32")]
    pub(super) books: Mutex<super::book::browser::BookResources>,
    pub(crate) state: Arc<RwLock<LibraryState>>,
    pub(crate) activity: LibraryActivity,
    /// Policy changes drain active evictions without reserving library state.
    pub(crate) evictions: RwLock<()>,
    /// File snapshots share access; root replacement exclusively drains them.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) file_access: Arc<RwLock<()>>,
    /// Retain the file pause across failed recovery attempts.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) root_pause: tokio::sync::Mutex<Option<super::lifecycle::RootPause>>,
    pub(crate) upload_quota_blocked: AtomicBool,
    pub(crate) account: LibraryAccount,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) root_failures: Option<async_channel::Sender<LibraryId>>,
    pub(crate) cpu: Arc<dyn CpuHost>,
    /// Local filesystem discovery and inspection.
    pub(crate) scanning: AtomicBool,
}

impl LibraryResources {
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) async fn open_assets(root: String, database: &DatabaseClient) -> Result<AssetStore, BackendError> {
        use content_address::hash_file;
        use library_files::import_move::recover_import_moves;
        use std::fs::File;
        use std::io;
        use std::path::Path;

        Ok(database
            .run(move |database| {
                let canonical = Path::new(&root).canonicalize().map_err(DatabaseError::operation)?;
                let unresolved = recover_import_moves(&canonical, |path| {
                    let checksum = hash_file(&mut File::open(path)?)?;
                    let Some(book) = database.book_for_local_content(checksum).map_err(io::Error::other)? else { return Ok(false) };
                    Ok(database.book_paths(&book).map_err(io::Error::other)?.into_iter().any(|relative| canonical.join(relative.as_str().trim_start_matches('/')) == path))
                })
                .map_err(DatabaseError::operation)?;
                for error in unresolved {
                    log::warn!("interrupted import move needs recovery: {error}");
                }
                AssetStore::open(&root).map_err(DatabaseError::operation)
            })
            .await?)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn watch_root(&self, assets: &AssetStore) {
        if let Some(failures) = self.root_failures.clone() {
            let id = self.id;
            assets.on_root_unavailable(move || {
                let _ = failures.try_send(id);
            });
        }
    }

    pub(crate) async fn open(options: LibraryOpenOptions, executor: &BackendExecutor) -> Result<Self, BackendError> {
        // Open and initialize storage before sharing its library state lock.
        let database = DatabaseClient::open(&options.database_locator, executor).await?;
        let cpu = options.cpu;
        #[cfg(not(target_arch = "wasm32"))]
        let assets = Self::open_assets(options.root.clone(), &database).await?;
        #[cfg(target_arch = "wasm32")]
        let assets = crate::asset_store::open(&options.root, cpu.clone()).map_err(BackendError::operation)?;
        // `DatabaseClient::open` already configured the connection (pragmas, sync
        // functions) internally.
        database.initialize_library_for_deferred_taxonomy(options.id).await?;
        #[cfg(not(target_arch = "wasm32"))]
        database.apply_native_root_move(&options.root).await?;
        let (events, event_state) = LibraryEventRouter::new();
        database.set_storage_policy(&options.storage, options.reset_cloud_presence).await?;
        let state = Arc::new(RwLock::new(LibraryState::new(database.clone(), assets, events.clone())));
        let authentication = LibraryAuthentication::Account(options.account.clone());
        let transfers = AssetExecutor::new(state.clone(), database.clone(), events.clone(), options.id, authentication.clone(), options.notification_interest.clone(), cpu.clone());
        transfers.set_asset_storage_enabled(options.storage.uses_bokheim_assets());
        let sync = Arc::new(
            LibrarySync::new(LibrarySyncConfig {
                database: database.clone(),
                events: events.clone(),
                http_client: transfers.http_client().clone(),
                authentication,
                // Opening already knows the configured server. Defer account
                // authorization until remote work actually needs credentials.
                server_url: options.default_sync_server_url,
                library_id: options.id,
                notification_interest: options.notification_interest,
            })
            .await
            .map_err(BackendError::operation)?,
        );
        let client = MetadataClient::new(&options.metadata_server_url).map_err(BackendError::operation)?;
        let covers = CoverClient::default();
        let enrichment = LibraryEnrichment::new(state.clone(), database.clone(), client, covers, cpu.clone(), events.clone());
        let library = Self {
            database,
            id: options.id,
            enrichment,
            sync,
            transfers,
            state,
            activity: LibraryActivity::new(),
            evictions: RwLock::new(()),
            #[cfg(not(target_arch = "wasm32"))]
            file_access: Arc::new(RwLock::new(())),
            #[cfg(not(target_arch = "wasm32"))]
            root_pause: tokio::sync::Mutex::new(None),
            events,
            event_state: Mutex::new(event_state),
            #[cfg(target_arch = "wasm32")]
            books: Mutex::default(),
            upload_quota_blocked: AtomicBool::new(false),
            account: options.account,
            #[cfg(not(target_arch = "wasm32"))]
            root_failures: options.root_failures,
            cpu,
            scanning: AtomicBool::new(false),
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            if options.reset_local_presence {
                let storage = library.state.write().await;
                library.reset_local_storage(&storage).await?;
            }
            library.watch_root(library.state.read().await.assets());
        }
        Ok(library)
    }

    /// Discard file observations and notify affected books after the database
    /// reset succeeds. The caller retains the library state write lock.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) async fn reset_local_storage(&self, storage: &LibraryState) -> Result<(), BackendError> {
        for hash in storage.database().reset_local_storage().await? {
            self.events.download_status_changed(hash, DownloadState::NotDownloaded);
        }
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::library::lifecycle::concurrency_tests::fixture;
    use sync_common::ROOT_DIR_ID;

    #[tokio::test]
    async fn opening_library_recovers_an_abandoned_legacy_claim_before_scanning() {
        use serde_json::json;
        use std::fs;
        let root = tempfile::tempdir().unwrap();
        let original = tempfile::tempdir().unwrap();
        let source = original.path().join("book.epub");
        let journal = root.path().join("Nested/.bokheim-import-move-restart-test");
        fs::create_dir_all(&journal).unwrap();
        let staged = journal.join("book.epub");
        fs::write(&staged, b"unpublished book").unwrap();
        fs::write(journal.join("recovery.json"), serde_json::to_vec(&json!({"source":source,"staged":staged,"destination":null})).unwrap()).unwrap();
        let (_service, _handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
        assert_eq!(fs::read(&source).unwrap(), b"unpublished book");
        assert!(!journal.exists());
    }

    #[tokio::test]
    async fn reopening_library_keeps_a_committed_move_when_cleanup_was_interrupted() {
        use library_files::import_move::ImportMove;
        use std::fs;
        let root = tempfile::tempdir().unwrap();
        let original = tempfile::tempdir().unwrap();
        let source = original.path().join("book.epub");
        fs::write(&source, include_bytes!("../../tests/fixtures/cloud-cover.epub")).unwrap();
        let (service, handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
        let moved = service.import(&*service.state.write().await).move_book_from_path(ROOT_DIR_ID, &source).await.unwrap();
        let name = service.state.read().await.database().transfer_destination_name(&moved.book_id, &ROOT_DIR_ID).await.unwrap().unwrap();
        let destination = root.path().join(name).canonicalize().unwrap();
        // Recreate the committed publication with a real indexed claim, then
        // leave its journal as if the process exited before private cleanup.
        fs::rename(&destination, &source).unwrap();
        let mut claim = ImportMove::claim_in_library(&source, root.path(), root.path()).unwrap();
        let journal = claim.path().parent().unwrap().to_owned();
        assert_eq!(claim.publish(root.path(), &[]).unwrap(), destination);
        claim.mark_committed();
        drop(claim);
        drop(service);
        drop(handle);
        let (service, _handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
        assert!(!source.exists());
        assert!(!journal.exists());
        assert!(destination.is_file());
        assert_eq!(service.state.read().await.database().book_count().await.unwrap(), 1);
    }
}
