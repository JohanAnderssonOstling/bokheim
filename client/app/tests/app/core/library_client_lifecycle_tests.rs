use super::*;
use crate::app::data::AppDataLocation;
use std::time::Duration;

impl AppBackend {
    pub(in crate::app) fn account_test_backend(context: BackendContext, server: account_client::Url) -> Self {
        let account = DeviceAccountSession::load(SessionStorage::new(context.registry.clone()), server).unwrap();
        let backend = Self::with_account(context, account).unwrap();
        let mut state = Arc::try_unwrap(backend.state).unwrap_or_else(|_| panic!("no active account maintenance"));
        state.account_service.shutdown();
        state.root_task.take();
        Self { state: Arc::new(state) }
    }
}

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
            assert_eq!(data.contents.books.iter().filter_map(|book| book.book_id()).collect::<Vec<_>>(), [downloaded], "only downloaded books should be displayed");
        }
        let section = library.browse_section_books(query(), 0, 8).await.unwrap();
        assert_eq!(section.books.iter().filter_map(|book| book.book_id()).collect::<Vec<_>>(), [downloaded]);
    }

    let root = tempfile::tempdir().unwrap();
    let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
    let (backend, quota_server) = super::quota_tests::quota_backend(context).await;
    let session = |user: &str, token: char| account_client::Session::try_new(quota_server.url.clone(), user.into(), format!("{user}@example.com"), token.to_string().repeat(43)).unwrap();
    backend.state.account_service.adopt_session(session("first", 'a')).unwrap();
    let id = LibraryId::new_v4();
    backend.state.library_service.attach_remote_libraries(&[sync_common::api::libraries::LibrarySummary { library_id: id, library_name: "Retained".into(), storage: Default::default(), kind: Default::default() }]).unwrap();
    let entry = backend.library_entry(&id).unwrap();
    let library = LibraryClient::new(id, backend.state.library_directory.get(&id).unwrap());
    let hash = library.import_downloaded_book(ROOT_DIR_ID, "Book.epub".into(), include_bytes!("../../../../library-backend/tests/fixtures/cloud-cover.epub").to_vec()).await.unwrap();
    let remote_hash = library.import_downloaded_book(ROOT_DIR_ID, "Remote.epub".into(), include_bytes!("../../../../library-backend/crates/book-inspection/tests/fixtures/malformed-language.epub").to_vec()).await.unwrap();
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
    let credentials = backend.library_runtime_spec(id).unwrap().0.account.credentials();
    assert_eq!(credentials.credentials().unwrap().access_token(), "a".repeat(43));

    backend.state.account_service.logout_with_notification(|_| async {}).unwrap();
    assert_eq!(backend.libraries().unwrap().len(), 1);
    assert!(credentials.credentials().is_none());
    assert!(library.resolve_book(hash).await.is_ok(), "downloaded books remain readable while signed out");
    assert_downloaded_browse(&library, hash).await;
    assert!(backend.set_library_asset_storage(id, false).await.is_err(), "cloud removal must require the owner to sign in before downloading remaining books");
    assert!(backend.state.library_service.library_asset_storage_enabled(&id).unwrap());
    assert!(backend.state.registry.read_value(&format!("pending-cloud-storage:{id}")).unwrap().is_none());
    assert!(library.browse_result_page(query(), online_revision, BrowsePageRequest::default()).await.unwrap().is_none(), "an old signed-in page cannot expose server books after logout");

    backend.state.account_service.adopt_session(session("second", 'b')).unwrap();
    assert!(backend.libraries().unwrap().is_empty(), "another account must not see the first account's libraries");
    assert!(credentials.credentials().is_none());
    assert!(library.resolve_book(hash).await.is_err(), "previous clients must not open even downloaded books under another account");
    assert!(library.browse(query()).await.is_err());
    assert!(backend.library_entry(&id).is_err());
    assert!(backend.delete_library(&id).await.is_err());
    assert!(backend.state.registry.pending_server_deletions().unwrap().is_empty());
    let second_id = LibraryId::new_v4();
    backend.state.library_service.attach_remote_libraries(&[sync_common::api::libraries::LibrarySummary { library_id: second_id, library_name: "Second account".into(), storage: Default::default(), kind: Default::default() }]).unwrap();
    assert_eq!(backend.libraries().unwrap().iter().map(|entry| *entry.library_id()).collect::<Vec<_>>(), [second_id]);
    let second = LibraryClient::new(second_id, backend.state.library_directory.get(&second_id).unwrap());

    backend.state.account_service.adopt_session(session("first", 'c')).unwrap();
    assert_eq!(credentials.credentials().unwrap().access_token(), "c".repeat(43));
    assert_eq!(backend.libraries().unwrap().iter().map(|entry| *entry.library_id()).collect::<Vec<_>>(), [id]);
    let restored = LibraryClient::new(id, backend.state.library_directory.get(&id).unwrap());
    assert!(restored.resolve_book(hash).await.is_ok());
    assert_eq!(restored.browse(query()).await.unwrap().contents.books.len(), 2);
    assert!(library.book_count().await.is_err(), "retired clients cannot reconnect to a new account session");
    assert!(second.book_count().await.is_err());

    backend.state.account_service.logout_with_notification(|_| async {}).unwrap();
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
        let mut changes = client.management.library_list_changes().await.unwrap();
        let mut other = client.management.library_list_changes().await.unwrap();
        assert!(!changes.has_changed().unwrap());

        let entry = client.management.create_library_with_asset_storage("Books".to_owned(), false).await.unwrap();
        let id = *entry.library_id();
        changes.changed().await.unwrap();
        assert!(other.has_changed().unwrap());
        other.changed().await.unwrap();
        assert_eq!(client.management.libraries().await.unwrap().len(), 1);

        client.rename_library(id, "First name".to_owned()).await.unwrap();
        client.rename_library(id, "Final name".to_owned()).await.unwrap();
        changes.changed().await.unwrap();
        assert!(!changes.has_changed().unwrap(), "unread changes should coalesce");
        assert!(other.has_changed().unwrap(), "each subscriber tracks changes independently");
        assert_eq!(client.management.libraries().await.unwrap()[0].library_name(), "Final name");

        drop(other);
        client.delete_library(id).await.unwrap();
        changes.changed().await.unwrap();
        assert!(client.management.libraries().await.unwrap().is_empty());
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
    let id = *backend.state.library_service.ensure_import_library().unwrap().library_id();
    let (client, worker) = crate::runtime::native::channel(&backend);
    // The app worker is deliberately never served. Library requests must
    // go straight to the already-running library actor.
    let old = client.management.library(id);
    assert_eq!(tokio::time::timeout(Duration::from_secs(5), old.book_count()).await.unwrap().unwrap(), 0);
    let (updates, _) = old.updates().await.unwrap();
    backend.library_directory().retire(&id).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async { while updates.recv().await.is_ok() {} }).await.expect("retirement must close subscriptions even while the old client is retained");
    assert!(tokio::time::timeout(Duration::from_secs(5), old.book_count()).await.unwrap().is_err());
    backend.open_registered_library(&id).unwrap();
    let fresh = client.management.library(id);
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

    let destination = backend.state.library_service.ensure_import_library().unwrap();
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
                *backend.state.library_service.ensure_import_library().unwrap().library_id()
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
    let id = *backend.state.library_service.ensure_import_library().unwrap().library_id();
    let (client, worker) = crate::runtime::native::channel(&backend);
    let library = client.management.library(id);
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
