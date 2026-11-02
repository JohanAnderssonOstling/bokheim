#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) use super::data::AppDataLocation;
pub(super) use super::data::{BackendContext, LibraryEntry, LibraryRegistry};
use crate::AppStartupState;
use crate::executor::{BackendExecutor, BackendOutput, BackendSend, BackendTask};
use crate::host::startup_profile::StartupProfile;
#[cfg(not(feature = "kobo"))]
use client_platform_runtime::transfers;
use library_backend::library::LibraryDirectory;
use serde::Serialize;
pub(super) use std::collections::HashSet;
pub(super) use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(all(test, not(target_arch = "wasm32")))]
use std::time::Duration;
pub(super) use sync_common::LibraryId;
use tokio::sync::watch;

pub(super) use crate::DeviceAccountSession;
pub(super) use crate::{BackendError, Failure};
pub(super) use library_backend::{LibrarySyncState, LibrarySyncStatus};

#[derive(Clone, Debug, serde::Deserialize, Serialize)]
pub struct LibraryListState {
    pub libraries: Vec<LibraryEntry>,
    pub sync_statuses: Vec<LibrarySyncStatus>,
}

pub(crate) struct AppState {
    pub(super) dictionaries: super::dictionaries::DictionaryStore,
    pub(super) wikipedia_client: tokio::sync::OnceCell<Option<reqwest::Client>>,
    pub(super) translation_client: tokio::sync::OnceCell<Option<reqwest::Client>>,
    pub(super) executor: BackendExecutor,
    pub(super) registry: LibraryRegistry,
    pub(super) storage_copy_lock: tokio::sync::Mutex<()>,
    pub(super) cloud_quota_lock: Arc<tokio::sync::Mutex<()>>,
    /// Durable registry/account policy remain app-owned. The library module
    /// owns the in-memory directory of resource-bearing sessions.
    pub(super) library_directory: Arc<LibraryDirectory>,
    pub(super) account: DeviceAccountSession,
    pub(super) library_list_changed: watch::Sender<()>,
    /// Coalesces requests for a full account-maintenance pass.
    pub(super) account_wake: watch::Sender<()>,
    account_task: Option<BackendTask<()>>,
    #[cfg(not(target_arch = "wasm32"))]
    root_task: Option<BackendTask<()>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) root_failures: async_channel::Sender<LibraryId>,
    pub(super) notification_interest: crate::notification_interest::NotificationInterest,
}

impl Drop for AppState {
    fn drop(&mut self) {
        self.account_task.take();
        #[cfg(not(target_arch = "wasm32"))]
        self.root_task.take();
        self.library_directory.shutdown();
    }
}

#[derive(Clone)]
pub(crate) struct AppBackend {
    pub(super) state: Arc<AppState>,
}

impl AppBackend {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(super) fn account_test_backend(context: BackendContext, server: account_client::Url) -> Self {
        let backend = Self::new_without_libraries(context.clone()).unwrap();
        let mut state = Arc::try_unwrap(backend.state).unwrap_or_else(|_| panic!("no active account maintenance"));
        state.account_task.take();
        state.root_task.take();
        state.account = DeviceAccountSession::load(context, server).unwrap();
        Self { state: Arc::new(state) }
    }

    /// The storage owner applies registry policy and the corresponding
    /// directory or account changes together, before publishing the result.
    pub(super) fn with_storage<T: BackendOutput>(&self, operation: impl FnOnce(&Self) -> Result<T, BackendError> + BackendSend + 'static) -> Result<T, BackendError> {
        use client_platform_runtime::storage_queue::{self, Priority};
        let this = self.clone();
        storage_queue::run_sync(Priority::Interactive, move || operation(&this)).map_err(BackendError::message)?
    }

    pub(crate) fn startup_state(&self) -> Result<AppStartupState, BackendError> {
        Ok(AppStartupState {
            libraries: self.libraries().map_err(BackendError::operation)?,
            browsing_preferences: self.browsing_preferences().map_err(BackendError::operation)?,
            account_status: self.account_status().map_err(BackendError::operation)?,
            reader_preferences: self.reader_preferences().map_err(BackendError::operation)?,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn run_native_worker(self, worker: crate::runtime::native::AppWorker) {
        let executor = self.state.executor.clone();
        executor.block_on(worker.serve(self));
    }

    pub fn new(context: BackendContext) -> Result<Self, BackendError> {
        let core = Self::new_without_libraries(context)?;
        core.open_startup_libraries()?;
        Ok(core)
    }

    pub(crate) fn new_without_libraries(context: BackendContext) -> Result<Self, BackendError> {
        let mut profile = StartupProfile::new("backend_construct");
        let executor = BackendExecutor::new().map_err(BackendError::operation)?;
        profile.mark("executor");
        let notification_interest = crate::notification_interest::NotificationInterest::default();
        let library_directory = Arc::new(LibraryDirectory::default());
        let (account_wake, account_requests) = watch::channel(());
        let api_server = crate::api_server_url();
        let account = DeviceAccountSession::load(context.clone(), api_server).map_err(BackendError::operation)?;
        profile.mark("account_load");
        #[cfg(not(target_arch = "wasm32"))]
        let (root_failures, root_requests) = async_channel::unbounded();
        let dictionaries = super::dictionaries::new(context.clone());
        let state = Arc::new_cyclic(|weak| AppState {
            dictionaries,
            wikipedia_client: Default::default(),
            translation_client: Default::default(),
            account_task: Some(super::account::start(&executor, weak, account.clone(), account_wake.clone(), account_requests, notification_interest.clone(), library_directory.clone())),
            #[cfg(not(target_arch = "wasm32"))]
            root_task: Some(super::root_recovery::start(&executor, weak.clone(), root_requests)),
            #[cfg(not(target_arch = "wasm32"))]
            root_failures,
            executor: executor.clone(),
            registry: context.registry,
            storage_copy_lock: Default::default(),
            cloud_quota_lock: Default::default(),
            library_directory,
            account: account.clone(),
            library_list_changed: watch::channel(()).0,
            account_wake,
            notification_interest: notification_interest.clone(),
        });
        let core = Self { state };
        if let Some(account) = core.account_identity()? {
            // Libraries created locally while signed out can now synchronize.
            // Legacy caches wait for discovery to establish their ownership.
            core.state.registry.bind_unowned_libraries(&account).map_err(BackendError::operation)?;
        }
        profile.mark("state_and_account_task");
        let browsing = core.browsing_preferences()?;
        client_platform_runtime::text_search::set_enabled(browsing.text_search());
        #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
        transfers::initialize(&context.app_data_dir, browsing.allow_metered_transfers(), &executor).map_err(BackendError::operation)?;
        #[cfg(target_arch = "wasm32")]
        transfers::POLICY.set_allow_metered(browsing.allow_metered_transfers());
        profile.mark("transfer_policy");
        // Catalog queries share one disk database; activate it before libraries open.
        super::catalog::start(&context.app_data_dir, &executor, core.state.library_directory.clone());
        profile.mark("catalog_activate");
        Ok(core)
    }

    pub(crate) fn open_startup_libraries(&self) -> Result<(), BackendError> {
        let mut profile = StartupProfile::new("library_startup");
        self.open_registered_libraries()?;
        profile.mark("libraries_open");
        self.start_pending_local_purges();
        self.state.account_wake.send_replace(());
        profile.mark("maintenance_schedule");
        Ok(())
    }

    pub(crate) fn library_list_changes(&self) -> watch::Receiver<()> {
        self.state.library_list_changed.subscribe()
    }

    pub(crate) fn notification_interest(&self, browsing: bool) -> async_channel::Receiver<()> {
        let (sender, receiver) = async_channel::bounded(1);
        let interest = self.state.notification_interest.view(browsing);
        self.state.executor.spawn_detached(async move {
            let _interest = interest;
            sender.closed().await;
        });
        receiver
    }

    pub(super) fn send_library_list_changed(&self) {
        self.state.library_list_changed.send_replace(());
    }

    /// Runtime routing may resolve an already-open actor here. It cannot open
    /// or configure libraries; that remains the explicit lifecycle path.
    pub(crate) fn library_directory(&self) -> &Arc<LibraryDirectory> {
        &self.state.library_directory
    }

    pub(super) fn library_entry(&self, library_id: &LibraryId) -> Result<LibraryEntry, BackendError> {
        if !self.state.registry.accessible_to_account(library_id, self.account_identity()?.as_deref()).map_err(BackendError::operation)? {
            return Err(BackendError::classified(Failure::SignInRequired, "sign in to the account that owns this library"));
        }
        let entry = self.state.registry.entry(library_id).map_err(BackendError::operation)?.ok_or_else(|| BackendError::message(format!("library not found: {library_id}")))?;
        #[cfg(not(target_arch = "wasm32"))]
        if entry.is_opds() {
            self.opds_access(*library_id)?;
        }
        Ok(entry)
    }
}

pub(super) fn library_name_for_folder(path: &Path) -> String {
    path.file_name().and_then(|name| name.to_str()).filter(|name| !name.trim().is_empty()).map(str::to_owned).unwrap_or_else(|| path.display().to_string())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod library_client_lifecycle_tests {
    use super::*;

    #[tokio::test]
    async fn cloud_libraries_are_available_signed_out_but_hidden_from_other_accounts() {
        use library_backend::LibraryClient;
        use library_model::{BlobKind, BrowseLocation, BrowsePageRequest, LibraryBrowseQuery};
        use sync_common::ROOT_DIR_ID;

        fn query() -> LibraryBrowseQuery {
            LibraryBrowseQuery { location: BrowseLocation::Books, search: String::new(), file_types: Vec::new(), languages: Vec::new(), chip_sort: Default::default(), book_sort: Default::default(), hide_finished: false }
        }

        async fn assert_downloaded_browse(library: &LibraryClient, downloaded: sync_common::BookId) {
            for data in [library.browse(query()).await.unwrap(), library.browse_page(query(), BrowsePageRequest::default()).await.unwrap()] {
                assert_eq!(data.contents.books.iter().filter_map(|book| book.content_hash()).collect::<Vec<_>>(), [downloaded], "only downloaded books should be displayed");
            }
            let section = library.browse_section_books(query(), 0, 8).await.unwrap();
            assert_eq!(section.books.iter().filter_map(|book| book.content_hash()).collect::<Vec<_>>(), [downloaded]);
        }

        let root = tempfile::tempdir().unwrap();
        let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
        let (backend, quota_server) = super::super::quota::tests::quota_backend(context).await;
        let session = |user: &str, token: char| account_client::Session::try_new(quota_server.url.clone(), user.into(), format!("{user}@example.com"), token.to_string().repeat(43)).unwrap();
        backend.adopt_session(session("first", 'a')).unwrap();
        let id = LibraryId::new_v4();
        backend.attach_remote_libraries(&[sync_common::api::libraries::LibrarySummary { library_id: id, library_name: "Retained".into(), storage: Default::default() }]).unwrap();
        let entry = backend.library_entry(&id).unwrap();
        let library = LibraryClient::new(id, backend.state.library_directory.get(&id).unwrap());
        let hash = library.import_downloaded_book(ROOT_DIR_ID, "Book.epub".into(), include_bytes!("../../../library-backend/tests/fixtures/cloud-cover.epub").to_vec()).await.unwrap();
        let remote_hash = library.import_downloaded_book(ROOT_DIR_ID, "Remote.epub".into(), include_bytes!("../../../library-backend/crates/book-inspection/tests/fixtures/malformed-language.epub").to_vec()).await.unwrap();
        // Retain a server book's catalog entry while removing its local copy.
        backend.state.library_directory.retire(&id).unwrap();
        {
            let database = library_database::Database::open(&backend.state.registry.library_database_locator(&id, entry.storage_locator())).unwrap();
            database.record_remote_asset(BlobKind::Book, &remote_hash).unwrap();
            database.evict_local_book(&remote_hash).unwrap();
            std::fs::remove_file(Path::new(entry.storage_locator()).join("Remote.epub")).unwrap();
        }
        backend.open_registered_library(&id).unwrap();
        let library = LibraryClient::new(id, backend.state.library_directory.get(&id).unwrap());
        let online = library.browse_page(query(), BrowsePageRequest::default()).await.unwrap();
        assert_eq!(online.contents.books.len(), 2, "the owner can browse downloaded and server books");
        let online_revision = online.page.unwrap().result_revision.unwrap();
        let credentials = backend.library_account(id).credentials();
        assert_eq!(credentials.credentials().unwrap().access_token(), "a".repeat(43));

        backend.logout_with_notification(|_| async {}).unwrap();
        assert_eq!(backend.libraries().unwrap().len(), 1);
        assert!(credentials.credentials().is_none());
        assert!(library.resolve_book(hash).await.is_ok(), "downloaded books remain readable while signed out");
        assert_downloaded_browse(&library, hash).await;
        assert!(backend.set_library_asset_storage(id, false).await.is_err(), "cloud removal must require the owner to sign in before downloading remaining books");
        assert!(backend.library_asset_storage_enabled(&id).unwrap());
        assert!(backend.state.registry.read_value(&format!("pending-cloud-storage:{id}")).unwrap().is_none());
        assert!(library.browse_result_page(query(), online_revision, BrowsePageRequest::default()).await.unwrap().is_none(), "an old signed-in page cannot expose server books after logout");

        backend.adopt_session(session("second", 'b')).unwrap();
        assert!(backend.libraries().unwrap().is_empty(), "another account must not see the first account's libraries");
        assert!(credentials.credentials().is_none());
        assert!(library.resolve_book(hash).await.is_err(), "previous clients must not open even downloaded books under another account");
        assert!(library.browse(query()).await.is_err());
        assert!(backend.library_entry(&id).is_err());
        assert!(backend.delete_library(&id).await.is_err());
        assert!(backend.state.registry.pending_server_deletions().unwrap().is_empty());
        let second_id = LibraryId::new_v4();
        backend.attach_remote_libraries(&[sync_common::api::libraries::LibrarySummary { library_id: second_id, library_name: "Second account".into(), storage: Default::default() }]).unwrap();
        assert_eq!(backend.libraries().unwrap().iter().map(|entry| *entry.library_id()).collect::<Vec<_>>(), [second_id]);
        let second = LibraryClient::new(second_id, backend.state.library_directory.get(&second_id).unwrap());

        backend.adopt_session(session("first", 'c')).unwrap();
        assert_eq!(credentials.credentials().unwrap().access_token(), "c".repeat(43));
        assert_eq!(backend.libraries().unwrap().iter().map(|entry| *entry.library_id()).collect::<Vec<_>>(), [id]);
        let restored = LibraryClient::new(id, backend.state.library_directory.get(&id).unwrap());
        assert!(restored.resolve_book(hash).await.is_ok());
        assert_eq!(restored.browse(query()).await.unwrap().contents.books.len(), 2);
        assert!(library.book_count().await.is_err(), "retired clients cannot reconnect to a new account session");
        assert!(second.book_count().await.is_err());

        backend.logout_with_notification(|_| async {}).unwrap();
        assert_eq!(backend.libraries().unwrap().len(), 2, "cached libraries remain available when signed out");
        assert!(restored.resolve_book(hash).await.is_ok());
        assert_downloaded_browse(&restored, hash).await;
    }

    #[tokio::test]
    async fn library_list_watch_tracks_lifecycle_for_independent_subscribers() {
        let root = tempfile::tempdir().unwrap();
        let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).unwrap();
        let (client, worker) = crate::runtime::native::channel(&backend);
        let exercise = async move {
            let mut changes = client.library_list_changes().await.unwrap();
            let mut other = client.library_list_changes().await.unwrap();
            assert!(!changes.has_changed().unwrap());

            let entry = client.create_library_with_asset_storage("Books".to_owned(), false).await.unwrap();
            let id = *entry.library_id();
            changes.changed().await.unwrap();
            assert!(other.has_changed().unwrap());
            other.changed().await.unwrap();
            assert_eq!(client.libraries().await.unwrap().len(), 1);

            client.rename_library(id, "First name".to_owned()).await.unwrap();
            client.rename_library(id, "Final name".to_owned()).await.unwrap();
            changes.changed().await.unwrap();
            assert!(!changes.has_changed().unwrap(), "unread changes should coalesce");
            assert!(other.has_changed().unwrap(), "each subscriber tracks changes independently");
            assert_eq!(client.libraries().await.unwrap()[0].library_name(), "Final name");

            drop(other);
            client.delete_library(id).await.unwrap();
            changes.changed().await.unwrap();
            assert!(client.libraries().await.unwrap().is_empty());
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(worker.serve(backend), exercise);
        })
        .await
        .expect("library list subscriptions must remain live through creation, rename, and deletion");
    }

    #[tokio::test]
    async fn library_calls_bypass_the_app_worker_and_retired_clients_stay_retired() {
        let root = tempfile::tempdir().unwrap();
        let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).unwrap();
        let id = *backend.ensure_import_library().unwrap().library_id();
        let (client, worker) = crate::runtime::native::channel(&backend);
        // The app worker is deliberately never served. Library requests must
        // go straight to the already-running library actor.
        let old = client.library(id);
        assert_eq!(tokio::time::timeout(Duration::from_secs(5), old.book_count()).await.unwrap().unwrap(), 0);
        let (updates, _) = old.updates().await.unwrap();
        backend.library_directory().retire(&id).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async { while updates.recv().await.is_ok() {} }).await.expect("retirement must close subscriptions even while the old client is retained");
        assert!(tokio::time::timeout(Duration::from_secs(5), old.book_count()).await.unwrap().is_err());
        backend.open_registered_library(&id).unwrap();
        let fresh = client.library(id);
        assert_eq!(fresh.book_count().await.unwrap(), 0);
        assert!(old.book_count().await.is_err(), "an old client must not retarget to a new actor generation");
        drop(worker);
    }

    #[tokio::test]
    async fn import_destination_skips_a_registered_library_that_cannot_open() {
        let root = tempfile::tempdir().unwrap();
        let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
        let unavailable = context.registry.create("Unavailable", None).unwrap();
        let database = context.registry.library_database_locator(unavailable.library_id(), unavailable.storage_locator());
        std::fs::create_dir_all(&database).unwrap();
        let backend = AppBackend::new(context).unwrap();

        let destination = backend.ensure_import_library().unwrap();
        assert_ne!(destination.library_id(), unavailable.library_id());
        assert!(backend.library_directory().get(destination.library_id()).is_ok());
        assert_eq!(backend.libraries().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn concurrent_imports_choose_one_destination() {
        let root = tempfile::tempdir().unwrap();
        let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).unwrap();
        let start = Arc::new(std::sync::Barrier::new(6));
        let clients = (0..6)
            .map(|_| {
                let backend = backend.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    *backend.ensure_import_library().unwrap().library_id()
                })
            })
            .collect::<Vec<_>>();
        let ids = clients.into_iter().map(|client| client.join().unwrap()).collect::<Vec<_>>();
        assert!(ids.iter().all(|id| id == &ids[0]));
        assert_eq!(backend.libraries().unwrap().len(), 1);
        assert_eq!(backend.library_directory().all().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn shutdown_closes_libraries_while_transport_survives() {
        let root = tempfile::tempdir().unwrap();
        let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).unwrap();
        let id = *backend.ensure_import_library().unwrap().library_id();
        let (client, worker) = crate::runtime::native::channel(&backend);
        let library = client.library(id);
        let (updates, _) = library.updates().await.unwrap();
        drop(worker);
        drop(backend);
        tokio::time::timeout(Duration::from_secs(5), async { while updates.recv().await.is_ok() {} }).await.expect("application shutdown must close libraries even while a transport retains the directory");
        assert!(library.book_count().await.is_err());
    }

    #[tokio::test]
    async fn failed_local_cleanup_keeps_deleted_library_hidden_and_retryable() {
        let root = tempfile::tempdir().unwrap();
        let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
        let entry = context.registry.create("Books", None).unwrap();
        let id = *entry.library_id();
        let blocked_file = std::path::Path::new(entry.storage_locator()).join(".bokheim/library.db");
        std::fs::create_dir(&blocked_file).unwrap();
        let backend = AppBackend::new(context).unwrap();
        assert!(backend.library_directory().get(&id).is_ok());

        assert!(backend.delete_library(&id).await.is_err());
        assert!(backend.libraries().unwrap().is_empty());
        assert!(backend.library_directory().get(&id).is_err());
        assert_eq!(backend.state.registry.pending_local_purges().unwrap(), [id]);

        std::fs::remove_dir(&blocked_file).unwrap();
        backend.state.registry.complete_local_purge(&id).unwrap();
        assert!(backend.state.registry.pending_local_purges().unwrap().is_empty());
    }
}
