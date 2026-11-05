use super::*;
use crate::app::data::AppDataLocation;
use crate::{BackendError, Failure};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use sync_common::LibraryId;

impl AppBackend {
    pub(in crate::app) async fn account_test_backend(context: BackendContext, server: account_client::Url) -> Arc<Self> {
        let account = DeviceAccountSession::load(SessionStorage::new(context.registry.clone()), server).unwrap();
        let backend = Self::with_account(context, account).await.unwrap();
        let mut backend = Arc::try_unwrap(backend).unwrap_or_else(|_| panic!("no active account maintenance"));
        backend.account_service.shutdown();
        backend.root_task.take();
        Arc::new(backend)
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
    let session = |user: &str, token: char| account_client::Session::try_new(quota_server.url.clone(), user.into(), format!("{user}@example.com"), token.to_string().repeat(43), i64::MAX - 1, "r".repeat(43), i64::MAX).unwrap();
    backend.account_service.adopt_session(session("first", 'a')).await.unwrap();
    let id = LibraryId::new_v4();
    backend.library_service.attach_remote_libraries(&[sync_common::api::libraries::LibrarySummary { library_id: id, library_name: "Retained".into(), storage: Default::default() }]).await.unwrap();
    let entry = backend.library_service.entry(&id).await.unwrap();
    let library = LibraryClient::new(backend.library_service.handle(&id).unwrap(), Arc::new(()));
    let hash = library.import_downloaded_book(ROOT_DIR_ID, "Book.epub".into(), include_bytes!("../../../../library-backend/tests/fixtures/cloud-cover.epub").to_vec()).await.unwrap();
    let remote_hash = library.import_downloaded_book(ROOT_DIR_ID, "Remote.epub".into(), include_bytes!("../../../../library-backend/crates/book-inspection/tests/fixtures/malformed-language.epub").to_vec()).await.unwrap();
    // Retain a server book's catalog entry while removing its local copy.
    backend.library_service.retire(&id);
    {
        let database = library_database::Database::open(&backend.registry.library_database_locator(&id, entry.storage_locator())).unwrap();
        database.record_remote_asset(BlobKind::Book, &remote_hash).unwrap();
        database.evict_local_book(&remote_hash).unwrap();
        std::fs::remove_file(Path::new(entry.storage_locator()).join("Remote.epub")).unwrap();
    }
    backend.library_service.open(&id).await.unwrap();
    let library = LibraryClient::new(backend.library_service.handle(&id).unwrap(), Arc::new(()));
    let online = library.browse_page(query(), BrowsePageRequest::default()).await.unwrap();
    assert_eq!(online.contents.books.len(), 2, "the owner can browse downloaded and server books");
    let online_revision = online.page.unwrap().result_revision.unwrap();
    let credentials = backend.library_service.open_options(id).await.unwrap().0.account.credentials();
    assert_eq!(credentials.credentials().await.unwrap().access_token(), "a".repeat(43));

    backend.account_service.logout_with_notification(|_| async {}).await.unwrap();
    assert_eq!(backend.library_service.list().await.unwrap().len(), 1);
    assert!(credentials.credentials().await.is_none());
    assert!(library.resolve_book(hash).await.is_ok(), "downloaded books remain readable while signed out");
    assert_downloaded_browse(&library, hash).await;
    assert!(backend.library_service.set_library_asset_storage(id, false).await.is_err(), "cloud removal must require the owner to sign in before downloading remaining books");
    assert!(backend.library_service.library_asset_storage_enabled(&id).await.unwrap());
    assert!(backend.registry.read_value(&format!("pending-cloud-storage:{id}")).unwrap().is_none());
    assert!(library.browse_result_page(query(), online_revision, BrowsePageRequest::default()).await.unwrap().is_none(), "an old signed-in page cannot expose server books after logout");

    backend.account_service.adopt_session(session("second", 'b')).await.unwrap();
    assert!(backend.library_service.list().await.unwrap().is_empty(), "another account must not see the first account's libraries");
    assert!(credentials.credentials().await.is_none());
    assert!(library.resolve_book(hash).await.is_err(), "previous clients must not open even downloaded books under another account");
    assert!(library.browse(query()).await.is_err());
    assert!(backend.library_service.entry(&id).await.is_err());
    assert!(backend.library_service.delete_library(&id).await.is_err());
    assert!(backend.registry.pending_server_deletions().unwrap().is_empty());
    let second_id = LibraryId::new_v4();
    backend.library_service.attach_remote_libraries(&[sync_common::api::libraries::LibrarySummary { library_id: second_id, library_name: "Second account".into(), storage: Default::default() }]).await.unwrap();
    assert_eq!(backend.library_service.list().await.unwrap().iter().map(|entry| *entry.library_id()).collect::<Vec<_>>(), [second_id]);
    let second = LibraryClient::new(backend.library_service.handle(&second_id).unwrap(), Arc::new(()));

    backend.account_service.adopt_session(session("first", 'c')).await.unwrap();
    assert_eq!(credentials.credentials().await.unwrap().access_token(), "c".repeat(43));
    assert_eq!(backend.library_service.list().await.unwrap().iter().map(|entry| *entry.library_id()).collect::<Vec<_>>(), [id]);
    let restored = LibraryClient::new(backend.library_service.handle(&id).unwrap(), Arc::new(()));
    assert!(restored.resolve_book(hash).await.is_ok());
    assert_eq!(restored.browse(query()).await.unwrap().contents.books.len(), 2);
    assert!(library.book_count().await.is_err(), "retired clients cannot reconnect to a new account session");
    assert!(second.book_count().await.is_err());

    backend.account_service.logout_with_notification(|_| async {}).await.unwrap();
    assert_eq!(backend.library_service.list().await.unwrap().len(), 2, "cached libraries remain available when signed out");
    assert!(restored.resolve_book(hash).await.is_ok());
    assert_downloaded_browse(&restored, hash).await;
}

#[tokio::test]
async fn library_list_watch_tracks_lifecycle_for_independent_subscribers() {
    let root = tempfile::tempdir().unwrap();
    let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).await.unwrap();
    let client = crate::hosting::client(backend.clone(), None);
    let exercise = async move {
        let mut changes = client.management.library_list_changes();
        let mut other = client.management.library_list_changes();
        assert!(!changes.has_changed().unwrap());

        let entry = client.management.create_library_with_asset_storage("Books".to_owned(), false).await.unwrap();
        let id = *entry.library_id();
        changes.changed().await.unwrap();
        assert!(other.has_changed().unwrap());
        other.changed().await.unwrap();
        assert_eq!(client.management.libraries().await.unwrap().len(), 1);

        client.management.rename_library(id, "First name".to_owned()).await.unwrap();
        client.management.rename_library(id, "Final name".to_owned()).await.unwrap();
        changes.changed().await.unwrap();
        assert!(!changes.has_changed().unwrap(), "unread changes should coalesce");
        assert!(other.has_changed().unwrap(), "each subscriber tracks changes independently");
        assert_eq!(client.management.libraries().await.unwrap()[0].library_name(), "Final name");

        drop(other);
        client.management.delete_library(id).await.unwrap();
        changes.changed().await.unwrap();
        assert!(client.management.libraries().await.unwrap().is_empty());
    };
    tokio::time::timeout(Duration::from_secs(5), exercise).await.expect("library list subscriptions must remain live through creation, rename, and deletion");
}

#[tokio::test]
async fn library_calls_use_their_actor_and_retired_clients_stay_retired() {
    let root = tempfile::tempdir().unwrap();
    let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).await.unwrap();
    let id = *backend.library_service.ensure_import_library().await.unwrap().library_id();
    let client = crate::hosting::client(backend.clone(), None);
    // Library requests go straight to the already-running library actor.
    let old = client.management.library(id).unwrap();
    assert_eq!(tokio::time::timeout(Duration::from_secs(5), old.book_count()).await.unwrap().unwrap(), 0);
    let (updates, _) = old.updates().await.unwrap();
    backend.library_service.retire(&id);
    tokio::time::timeout(Duration::from_secs(5), async { while updates.recv().await.is_ok() {} }).await.expect("retirement must close subscriptions even while the old client is retained");
    assert!(tokio::time::timeout(Duration::from_secs(5), old.book_count()).await.unwrap().is_err());
    backend.library_service.open(&id).await.unwrap();
    let fresh = client.management.library(id).unwrap();
    assert_eq!(fresh.book_count().await.unwrap(), 0);
    assert!(old.book_count().await.is_err(), "an old client must not retarget to a new actor generation");
}

#[tokio::test]
async fn import_destination_skips_a_registered_library_that_cannot_open() {
    let root = tempfile::tempdir().unwrap();
    let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
    let unavailable = context.registry.create("Unavailable", None).unwrap();
    let database = context.registry.library_database_locator(unavailable.library_id(), unavailable.storage_locator());
    std::fs::create_dir_all(&database).unwrap();
    let backend = AppBackend::new(context).await.unwrap();

    let destination = backend.library_service.ensure_import_library().await.unwrap();
    assert_ne!(destination.library_id(), unavailable.library_id());
    assert!(backend.library_service.handle(destination.library_id()).is_ok());
    assert_eq!(backend.library_service.list().await.unwrap().len(), 2);
}

#[tokio::test]
async fn concurrent_imports_choose_one_destination() {
    let root = tempfile::tempdir().unwrap();
    let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).await.unwrap();
    let start = Arc::new(tokio::sync::Barrier::new(6));
    let clients = (0..6)
        .map(|_| {
            let backend = backend.clone();
            let start = start.clone();
            tokio::spawn(async move {
                start.wait().await;
                *backend.library_service.ensure_import_library().await.unwrap().library_id()
            })
        })
        .collect::<Vec<_>>();
    let mut ids = Vec::new();
    for client in clients {
        ids.push(client.await.unwrap());
    }
    assert!(ids.iter().all(|id| id == &ids[0]));
    assert_eq!(backend.library_service.list().await.unwrap().len(), 1);
    assert_eq!(backend.library_service.list().await.unwrap().into_iter().filter(|entry| backend.library_service.handle(entry.library_id()).is_ok()).count(), 1);
}

#[tokio::test]
async fn explicit_shutdown_closes_libraries_while_client_survives() {
    let root = tempfile::tempdir().unwrap();
    let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).await.unwrap();
    let id = *backend.library_service.ensure_import_library().await.unwrap().library_id();
    let client = crate::hosting::client(backend.clone(), None);
    let library = client.management.library(id).unwrap();
    let (updates, _) = library.updates().await.unwrap();
    backend.library_service.shutdown();
    drop(backend);
    tokio::time::timeout(Duration::from_secs(5), async { while updates.recv().await.is_ok() {} }).await.expect("explicit shutdown must close libraries even while a client retains the host");
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
    let backend = AppBackend::new(context).await.unwrap();
    assert!(backend.library_service.handle(&id).is_ok());

    assert!(backend.library_service.delete_library(&id).await.is_err());
    assert!(backend.library_service.list().await.unwrap().is_empty());
    assert!(backend.library_service.handle(&id).is_err());
    assert_eq!(backend.registry.pending_local_purges().unwrap(), [id]);

    std::fs::remove_dir(&blocked_file).unwrap();
    backend.registry.complete_local_purge(&id).unwrap();
    assert!(backend.registry.pending_local_purges().unwrap().is_empty());
}
