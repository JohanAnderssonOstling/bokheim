use super::super::library_resources::LibraryResources;
use super::*;
use crate::executor::BackendExecutor;
use crate::library::book::reading::thumbnail;
use crate::library::events::LibraryEvent;
use crate::library::LibraryNotifications;
use crate::{BookId, LibraryId, LibrarySyncState};
use library_services::interest::{InterestState, NotificationInterest};
use std::sync::Arc;
use std::time::Duration;
use sync_common::api::notifications::NotificationMessage;
use sync_common::ROOT_DIR_ID;
use web_time::Instant;

#[derive(Clone, Debug)]
struct Account(account_client::Session);
impl account_client::SessionPersistence for Account {
    fn load_account_session(&self) -> std::io::Result<Option<account_client::Session>> {
        Ok(Some(self.0.clone()))
    }
    fn save_account_session(&self, _: &account_client::Session) -> std::io::Result<()> {
        Ok(())
    }
    fn clear_account_session(&self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(in crate::library) async fn fixture(root: &std::path::Path, url: account_client::Url) -> (Arc<LibraryResources>, LibraryHandle) {
    let account = library_services::account::LibraryAccountSession::load(
        Account(account_client::Session::try_new(url.clone(), "user".into(), "reader@example.com".into(), "a".repeat(43), i64::MAX - 1, "r".repeat(43), i64::MAX).unwrap()),
        url.clone(),
    )
    .unwrap();
    let executor = BackendExecutor::new().unwrap();
    let session = LibraryResources::open(
        crate::library::LibraryOpenOptions {
            id: LibraryId::new_v4(),
            root: root.to_string_lossy().into_owned(),
            database_locator: root.join("library.sqlite").to_string_lossy().into_owned(),
            default_sync_server_url: url,
            metadata_server_url: "http://127.0.0.1:1".into(),
            storage: sync_common::storage::LibraryStorage::Bokheim,
            reset_cloud_presence: false,
            reset_local_presence: false,
            account: account.library_account(),
            cpu: Arc::new(cpu_host::NativeHost),
            notification_interest: Default::default(),
            root_failures: None,
        },
        &executor,
    )
    .await
    .unwrap();
    let service = Arc::new(session);
    let stopped = tokio::sync::watch::channel(false).0;
    let handle = LibraryHandle { owner: Arc::new(LibraryOwner { resources: service.clone(), stopped, workers: Mutex::new(None), executor }), host: None };
    (service, handle)
}

#[cfg(feature = "scanner")]
#[tokio::test]
async fn scanner_records_manual_files_without_cloud_admission() {
    let root = tempfile::tempdir().unwrap();
    let (service, _handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let bytes = include_bytes!("../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
    let path = root.path().join("Manual.epub");
    std::fs::write(&path, bytes).unwrap();

    service.scan_folder().await.unwrap();
    assert_eq!(service.state.read().await.database().book_count().await.unwrap(), 1);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(!service.scanner_running());

    let smaller = include_bytes!("../../crates/book-inspection/tests/fixtures/malformed-language.epub");
    let smaller_path = root.path().join("Small.epub");
    std::fs::write(&smaller_path, smaller).unwrap();
    service.scan_folder().await.unwrap();
    assert_eq!(service.state.read().await.database().book_count().await.unwrap(), 2);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(std::fs::read(&smaller_path).unwrap(), smaller);

    service.scan_folder().await.unwrap();
    assert_eq!(service.state.read().await.database().book_count().await.unwrap(), 2);
    assert_ne!(service.sync_status(true, Some(true)).await.unwrap().state, LibrarySyncState::CloudStorageFull);
}

#[tokio::test]
async fn cloud_and_local_only_imports_commit_without_cloud_admission() {
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let (service, _handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let bytes = include_bytes!("../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
    let source = external.path().join("Book.epub");
    std::fs::write(&source, bytes).unwrap();
    let failures = service.import(&*service.state.write().await).import_paths(ROOT_DIR_ID, vec![source.clone()], true).await.unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    assert!(!source.exists());
    assert_eq!(std::fs::read(root.path().join("Book.epub")).unwrap(), bytes);
    service.import(&*service.state.write().await).import_downloaded_book(ROOT_DIR_ID, "Downloaded.epub", bytes.to_vec()).await.unwrap();
    assert_eq!(service.state.read().await.database().book_count().await.unwrap(), 1);
    assert!(root.path().join("Downloaded.epub").exists());

    service.state.read().await.database().set_storage_policy(&sync_common::storage::LibraryStorage::Local, false).await.unwrap();
    service.import(&*service.state.write().await).import_downloaded_book(ROOT_DIR_ID, "Local.epub", bytes.to_vec()).await.unwrap();
    assert_eq!(service.state.read().await.database().book_count().await.unwrap(), 1);
}

#[cfg(feature = "scanner")]
#[tokio::test]
async fn failed_scan_releases_busy_state_and_can_scan_again() {
    use library_model::LibraryUpdate;

    let root = tempfile::tempdir().unwrap();
    let (service, _handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let updates = service.events.updates.subscribe();
    let directory = root.path().join("offline");
    std::fs::create_dir(&directory).unwrap();
    let marker = directory.join(".biblos_uuid");
    std::fs::write(&marker, "invalid directory marker").unwrap();

    assert!(service.scan_folder().await.is_err());
    assert!(!service.scanner_running(), "a failed scan must not block file replay");
    service.event_state.lock().expect("library events").drain(&service.events);
    let mut observed = Vec::new();
    while let Ok(update) = updates.try_recv() {
        observed.push(update);
    }
    assert!(observed.contains(&LibraryUpdate::Scanning(true)));
    assert!(observed.contains(&LibraryUpdate::Scanning(false)));

    std::fs::remove_file(marker).unwrap();
    service.scan_folder().await.unwrap();
    assert!(!service.scanner_running());
}

#[tokio::test]
async fn saving_a_reading_position_refreshes_the_library_before_delayed_sync() {
    use library_model::{BrowseBookSort, BrowseChipSort, BrowseLocation, HomeShelf, LibraryBrowseQuery};
    use sync_common::ROOT_DIR_ID;

    let root = tempfile::tempdir().unwrap();
    let (service, handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let hash = BookId::from(blake3::hash(b"reading-position wake"));
    service
        .state
        .read()
        .await
        .database()
        .commit_import(library_database::ImportCommit {
            source_identity: None,
            byte_checksum: None,
            parent_id: ROOT_DIR_ID,
            file_name: "Book.epub".into(),
            format: book_model::BookFormat::Epub,
            book_id: hash,
            size_bytes: 100,
            inspection_version: 1,
            inspection: Some(book_inspection::InspectedBook {
                pdf: None,
                audiobook: None,
                metadata: book_model::BookRecord { metadata: book_model::BookDetails { title: "Book".into(), ..Default::default() }, description: String::new() },
                toc: Vec::new(),
            }),
            published: library_model::PublishedStagedBook { name: "Book.epub".into(), relative_path: "/Book.epub".into(), published: false, fingerprint: None },
            request_thumbnail: false,
            restore_paths: Vec::new(),
        })
        .await
        .unwrap();
    let query = LibraryBrowseQuery {
        location: BrowseLocation::Home(Some(HomeShelf::RecentlyRead)),
        search: String::new(),
        file_types: Vec::new(),
        chip_sort: BrowseChipSort::DefaultOrder,
        book_sort: BrowseBookSort::DefaultOrder,
        languages: Vec::new(),
        hide_finished: false,
    };
    assert!(service.state.read().await.database().browse_contents(&query, false).await.unwrap().books.is_empty());
    service.event_state.lock().expect("library events").drain(&service.events);
    let asset_requests = service.events.assets.subscribe();
    let file_requests = service.events.files.subscribe();
    let mut requests = service.events.sync.subscribe();
    let updates = service.events.updates.subscribe();
    assert!(!requests.has_changed().unwrap());
    handle.update_reading_position(hash, "epubcfi(/6/2)".into(), Some(10.0), None).await.unwrap();
    let delay = requests.borrow_and_update().deadline.expect("position changes should schedule their own publication deadline").saturating_duration_since(web_time::Instant::now());
    assert!(delay <= Duration::from_secs(5) && delay > Duration::from_secs(4));
    assert_eq!(service.state.read().await.database().reading_position(&hash).await.unwrap().as_deref(), Some("epubcfi(/6/2)"));
    service.event_state.lock().expect("library events").drain(&service.events);
    assert_eq!(updates.try_recv().unwrap(), library_model::LibraryUpdate::ReadingProgress(hash));
    let recent = service.state.read().await.database().browse_contents(&query, false).await.unwrap().books;
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].book_id(), Some(hash));
    assert_eq!(recent[0].progress, 10.0);
    assert!(updates.is_empty());
    assert!(!asset_requests.has_changed().unwrap(), "saving progress must not start asset work");
    assert!(!file_requests.has_changed().unwrap(), "saving progress must not start file replay");
    let mut outbox = service.state.read().await.database().sync_prepare_outbox_excluding_positions(service.events.reading_positions.borrow().waiting(web_time::Instant::now())).await.unwrap();
    assert!(service.state.read().await.database().sync_next_outbox_page(&mut outbox).await.unwrap().iter().all(|p| p.state.value.kind() != "reading_position"));

    // Updates within a chapter must refresh too, without needing a completed chapter.
    handle.update_reading_position(hash, "epubcfi(/6/4)".into(), Some(20.0), None).await.unwrap();
    service.event_state.lock().expect("library events").drain(&service.events);
    assert_eq!(updates.try_recv().unwrap(), library_model::LibraryUpdate::ReadingProgress(hash));
    assert_eq!(service.state.read().await.database().browse_contents(&query, false).await.unwrap().books[0].progress, 20.0);
    handle.update_reading_position(hash, "epubcfi(/6/4)".into(), Some(20.0), None).await.unwrap();
    service.event_state.lock().expect("library events").drain(&service.events);
    assert!(updates.is_empty(), "an unchanged position should not reload the library");
}

#[tokio::test]
async fn thumbnails_load_by_hash_without_book_records_or_current_card_variants() {
    use crate::ThumbnailResolution;
    use library_model::BlobKind;

    let root = tempfile::tempdir().unwrap();
    let (service, _handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let session = &service;
    let hash = BookId::from(blake3::hash(b"existing book"));
    let master = b"existing cover";
    session.state.read().await.assets().write(BlobKind::Thumbnail, &hash, master).await.unwrap();
    for resolution in [ThumbnailResolution::Browse, ThumbnailResolution::CardHighDensity, ThumbnailResolution::HighDensity] {
        assert_eq!(thumbnail(&*session.state.read().await.assets(), hash, resolution).await.unwrap().as_deref(), Some(master.as_slice()));
    }
    assert_eq!(session.state.read().await.database().thumbnail_work_state(&hash).await.unwrap(), None);

    session.state.read().await.assets().write_thumbnail_set(&hash, master, b"browse card", b"dense card").await.unwrap();
    assert_eq!(thumbnail(&*session.state.read().await.assets(), hash, ThumbnailResolution::Browse).await.unwrap().as_deref(), Some(b"browse card".as_slice()));
    assert_eq!(thumbnail(&*session.state.read().await.assets(), hash, ThumbnailResolution::CardHighDensity).await.unwrap().as_deref(), Some(b"dense card".as_slice()));
    assert_eq!(thumbnail(&*session.state.read().await.assets(), hash, ThumbnailResolution::HighDensity).await.unwrap().as_deref(), Some(master.as_slice()));
}

#[tokio::test]
async fn remote_notifications_route_to_the_requested_library_and_reconnect_refreshes_all() {
    let first_root = tempfile::tempdir().unwrap();
    let second_root = tempfile::tempdir().unwrap();
    let (first_service, first) = fixture(first_root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let mut first_service_changes = first_service.events.sync.subscribe();
    let (second_service, second) = fixture(second_root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let mut second_service_changes = second_service.events.sync.subscribe();
    let mut libraries = vec![first.clone(), second.clone()];
    let notifications = LibraryNotifications::new(first_service.account.clone(), InterestState::default());

    notifications.receive(&NotificationMessage::LibraryChanged { library_id: *first.id() }, true, &libraries);
    first_service_changes.changed().await.unwrap();
    assert!(first_service_changes.borrow_and_update().refresh_remote);

    assert!(!second_service_changes.has_changed().unwrap());

    let hello = NotificationMessage::Hello { heartbeat_seconds: 5, timeout_seconds: 15 };
    notifications.receive(&hello, false, &libraries);

    for message in [hello, NotificationMessage::LibrariesChanged, NotificationMessage::Reconcile] {
        notifications.receive(&message, true, &libraries);
        first_service_changes.changed().await.unwrap();
        assert!(first_service_changes.borrow_and_update().refresh_remote);
        second_service_changes.changed().await.unwrap();
        assert!(second_service_changes.borrow_and_update().refresh_remote);
    }

    notifications.receive(&NotificationMessage::LibraryChanged { library_id: LibraryId::new_v4() }, true, &libraries);

    libraries.retain(|library| library.id() != first.id());
    first.shutdown();
    notifications.receive(&NotificationMessage::LibraryChanged { library_id: *first.id() }, true, &libraries);
    notifications.receive(&NotificationMessage::Reconcile, true, &libraries);
    assert!(!libraries.iter().any(|library| library.id() == first.id()));
    assert!(!first_service_changes.has_changed().unwrap());
    second_service_changes.changed().await.unwrap();
    assert!(second_service_changes.borrow_and_update().refresh_remote);
}

#[tokio::test]
async fn foreground_resume_reconciles_libraries_without_a_socket_and_does_not_poll() {
    let first_root = tempfile::tempdir().unwrap();
    let second_root = tempfile::tempdir().unwrap();
    let (first_service, first) = fixture(first_root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let mut first_service_changes = first_service.events.sync.subscribe();
    let (second_service, second) = fixture(second_root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let mut second_service_changes = second_service.events.sync.subscribe();
    let account = first_service.account.clone();
    let libraries = vec![first, second];
    let interest = NotificationInterest::default();
    let activity = interest.subscribe();
    let mut notifications = LibraryNotifications::new(account.clone(), *activity.borrow());
    let now = Instant::now();
    let view = interest.view(false);
    assert!(!activity.borrow().enabled(), "reader-only foreground activity needs no socket");
    assert!(!notifications.observe_activity_at(*activity.borrow(), now + Duration::from_secs(1), &libraries).await);

    // A deactivate/reactivate pair can coalesce into one activity update.
    drop(view);
    let view = interest.view(false);
    assert!(notifications.observe_activity_at(*activity.borrow(), now + Duration::from_secs(31), &libraries).await);
    first_service_changes.changed().await.unwrap();
    assert!(first_service_changes.borrow_and_update().refresh_remote);
    second_service_changes.changed().await.unwrap();
    assert!(second_service_changes.borrow_and_update().refresh_remote);
    assert!(!notifications.observe_activity_at(*activity.borrow(), now + Duration::from_secs(40), &libraries).await);

    drop(view);
    let view = interest.view(false);
    assert!(!notifications.observe_activity_at(*activity.borrow(), now + Duration::from_secs(45), &libraries).await, "rapid resumes are throttled");

    drop(view);
    let view = interest.view(false);
    assert!(notifications.observe_activity_at(*activity.borrow(), now + Duration::from_secs(65), &libraries).await);
    first_service_changes.changed().await.unwrap();
    assert!(first_service_changes.borrow_and_update().refresh_remote);
    second_service_changes.changed().await.unwrap();
    assert!(second_service_changes.borrow_and_update().refresh_remote);

    let token = account.require().unwrap().token().to_owned();
    assert!(account.refresh_after_unauthorized(token).await.unwrap().is_none());
    drop(view);
    let _view = interest.view(false);
    assert!(!notifications.observe_activity_at(*activity.borrow(), now + Duration::from_secs(100), &libraries).await, "signed-out resumes do not trigger discovery or library refresh");
}

#[tokio::test]
async fn subscription_registers_with_initial_state_and_receives_callback_events() {
    let root = tempfile::tempdir().unwrap();
    let (service, handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let session = service.clone();
    let _guard = session.activity.start(Some(library_model::LibraryBusyReason::Importing));
    let events = service.events.clone();
    let hash = BookId::from_bytes([3; 32]);
    let exercise = async {
        let (updates, scanning) = handle.updates().await.unwrap();
        assert!(!scanning, "foreground activity is separate from filesystem scanning");
        // Callback capabilities can cross threads without sharing service state.
        std::thread::spawn(move || events.cover_changed(hash)).join().unwrap();
        assert_eq!(
            [updates.recv().await.unwrap(), updates.recv().await.unwrap()],
            [library_model::LibraryUpdate::BookFileActivity(library_model::BookFileActivity::from_reasons(&[library_model::LibraryBusyReason::Importing], false)), library_model::LibraryUpdate::Covers(vec![hash])]
        );
        handle.shutdown();
        assert!(updates.recv().await.is_err());
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        exercise.await;
    })
    .await
    .expect("subscription and event delivery must make progress");
}

#[tokio::test]
async fn retirement_stops_buffered_updates_without_polling_workers() {
    use crate::library::subscriptions::LibraryUpdates;
    let root = tempfile::tempdir().unwrap();
    let (service, handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let updates = service.events.updates.subscribe();
    let updates = LibraryUpdates::new(updates, handle.clone());
    service.events.emit(LibraryEvent::ContentsChanged);
    handle.shutdown();
    assert_eq!(updates.try_recv(), Err(async_channel::TryRecvError::Closed));
    assert!(updates.recv().await.is_err());
    assert!(handle.updates().await.is_err());
}

#[tokio::test]
async fn retirement_cancels_a_file_action_waiting_for_root_recovery() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let root = tempfile::tempdir().unwrap();
    let (_service, handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let _moving = handle.owner.resources.file_access.write().await;
    let mut creating = std::pin::pin!(handle.create_directory(ROOT_DIR_ID, "Shelf".into()));
    assert!(matches!(creating.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Pending));
    handle.shutdown();
    assert!(tokio::time::timeout(Duration::from_secs(1), creating).await.unwrap().is_err());
}

#[tokio::test]
async fn mutations_publish_queued_events_through_background_processing() {
    let root = tempfile::tempdir().unwrap();
    let (service, handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let updates = service.events.updates.subscribe();
    let exercise = async {
        handle.create_directory(ROOT_DIR_ID, "Shelf".into()).await.unwrap();
        loop {
            if updates.recv().await.unwrap() == library_model::LibraryUpdate::Contents {
                break;
            }
        }
        handle.shutdown();
    };
    handle.start_workers();
    exercise.await;
}

#[tokio::test]
async fn folder_replay_follows_renames_with_background_workers() {
    let root = tempfile::tempdir().unwrap();
    let (service, handle) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    service.scanning.store(false, std::sync::atomic::Ordering::Release);
    let folder = service.state.write().await.create_directory(&sync_common::ROOT_DIR_ID, "Before").await.unwrap();
    library_placement::prepare_directory(&*service.state.read().await.assets(), &*service.state.read().await.database(), folder.id).await.unwrap();
    assert!(root.path().join("Before").is_dir());
    let exercise = async {
        for name in ["After", "Again"] {
            handle.rename_directory(folder.id, name.into()).await.unwrap();
            while !root.path().join(name).is_dir() {
                tokio::task::yield_now().await;
            }
        }
        assert!(!root.path().join("Before").exists());
        assert!(!root.path().join("After").exists());
        handle.shutdown();
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        handle.start_workers();
        exercise.await;
    })
    .await
    .expect("each rename reaches the disk");
}

#[tokio::test]
async fn browser_download_import_appears_on_home_before_enrichment() {
    use library_model::{BrowseLocation, HomeShelf, LibraryBrowseQuery, LibraryUpdate};
    use std::time::{SystemTime, UNIX_EPOCH};
    use sync_common::ROOT_DIR_ID;

    let library = tempfile::tempdir().unwrap();
    let downloads = tempfile::tempdir().unwrap();
    let source = downloads.path().join("download.epub");
    let bytes = include_bytes!("../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
    std::fs::write(&source, bytes).unwrap();
    let (service, _handle) = fixture(library.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let updates = service.events.updates.subscribe();
    let started = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()).unwrap();

    let failures = service.import(&*service.state.write().await).import_paths(ROOT_DIR_ID, vec![source.clone()], true).await.unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    let hash = service.state.read().await.database().book_for_local_content(blake3::hash(bytes).into()).await.unwrap().unwrap();
    assert!(!source.exists());
    assert_eq!(std::fs::read(library.path().join("download.epub")).unwrap(), bytes);
    service.event_state.lock().expect("library events").drain(&service.events);
    assert!(std::iter::from_fn(|| updates.try_recv().ok()).any(|update| update == LibraryUpdate::Contents));

    let mut query = LibraryBrowseQuery {
        location: BrowseLocation::Home(Some(HomeShelf::RecentlyAdded)),
        search: String::new(),
        file_types: Vec::new(),
        languages: Vec::new(),
        chip_sort: Default::default(),
        book_sort: Default::default(),
        hide_finished: false,
    };
    for downloaded_only in [false, true] {
        let page = service.state.read().await.database().browse(&query, downloaded_only).await.unwrap();
        assert_eq!(page.contents.books.len(), 1);
        let book = &page.contents.books[0];
        assert_eq!(book.book_id(), Some(hash));
        assert!(book.downloaded);
        assert!(book.added_at >= started);
    }
    query.location = BrowseLocation::Home(None);
    let home = service.state.read().await.database().browse(&query, false).await.unwrap();
    let recent = home.sections.iter().find(|section| section.parent.id == BrowseLocation::Home(Some(HomeShelf::RecentlyAdded))).unwrap();
    assert_eq!(recent.books[0].book_id(), Some(hash));
}

#[tokio::test]
async fn failed_external_imports_preserve_copies_or_restore_move_sources() {
    for moving in [false, true] {
        let library = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let source = external.path().join("broken.epub");
        std::fs::write(&source, b"invalid EPUB").unwrap();
        let (service, _handle) = fixture(library.path(), "http://127.0.0.1:1".parse().unwrap()).await;
        let shelf = service.state.write().await.create_directory(&sync_common::ROOT_DIR_ID, "Shelf").await.unwrap();
        let failures = service.import(&*service.state.write().await).import_paths(shelf.id, vec![source.clone()], moving).await.unwrap();
        assert_eq!(failures.len(), 1);
        assert!(source.exists());
        let moved = library.path().join("Shelf/broken.epub");
        assert_eq!(moved.exists(), !moving);
        let retry = if moving { source } else { moved };
        assert_eq!(std::fs::read(&retry).unwrap(), b"invalid EPUB");
        let failures = service.import(&*service.state.write().await).import_paths(shelf.id, vec![retry.clone()], true).await.unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(std::fs::read(&retry).unwrap(), b"invalid EPUB");
        assert!(!library.path().join("Shelf/broken 2.epub").exists());
    }
}

#[tokio::test]
async fn external_import_places_nested_books_before_registration() {
    for moving in [false, true] {
        let library = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let source = external.path().join("Books");
        std::fs::create_dir_all(source.join("Nested")).unwrap();
        let bytes = include_bytes!("../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
        std::fs::write(source.join("Nested/book.epub"), bytes).unwrap();
        std::fs::write(source.join("notes.txt"), b"sidecar").unwrap();
        let (service, _handle) = fixture(library.path(), "http://127.0.0.1:1".parse().unwrap()).await;
        let shelf = service.state.write().await.create_directory(&sync_common::ROOT_DIR_ID, "Shelf").await.unwrap();
        let failures = service.import(&*service.state.write().await).import_paths(shelf.id, vec![source.clone()], moving).await.unwrap();
        assert!(failures.is_empty(), "{failures:?}");
        assert_eq!(source.exists(), !moving);
        assert_eq!(std::fs::read(library.path().join("Shelf/Books/Nested/book.epub")).unwrap(), bytes);
        assert_eq!(std::fs::read(library.path().join("Shelf/Books/notes.txt")).unwrap(), b"sidecar");
        let hash = service.state.read().await.database().book_for_local_content(blake3::hash(bytes).into()).await.unwrap().unwrap();
        let mut paths: Vec<_> = service.state.read().await.database().book_paths(&hash).await.unwrap().iter().map(|path| path.to_string()).collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths, vec!["/Shelf/Books/Nested/book.epub"]);
        let failures = service.import(&*service.state.write().await).import_paths(shelf.id, vec![library.path().join("Shelf/Books")], true).await.unwrap();
        assert!(failures.is_empty(), "{failures:?}");
        assert!(!library.path().join("Shelf/Books 2").exists());
    }
}

#[tokio::test]
async fn selected_import_copies_all_files_before_shared_inspection() {
    use crate::{DirectoryImport, DirectoryImportFile, DirectoryImportReader, ImportSource};
    use std::io::Cursor;

    struct InterruptedReader(bool);
    impl std::io::Read for InterruptedReader {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            if self.0 {
                return Err(std::io::Error::other("copy interrupted"));
            }
            self.0 = true;
            let count = bytes.len().min(4);
            bytes[..count].copy_from_slice(&b"data"[..count]);
            Ok(count)
        }
    }

    let library = tempfile::tempdir().unwrap();
    let (service, _handle) = fixture(library.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let shelf = service.state.write().await.create_directory(&sync_common::ROOT_DIR_ID, "Shelf").await.unwrap();
    let bytes = include_bytes!("../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
    let destination = library.path().join("Shelf");
    let directory = DirectoryImport {
        name: "Selected books".into(),
        directories: vec![],
        files: vec![
            DirectoryImportFile { path: vec!["broken.epub".into()], source: ImportSource::new(Box::new(|| Box::pin(async { Ok(Box::new(Cursor::new(b"invalid EPUB")) as DirectoryImportReader) }))) },
            DirectoryImportFile {
                path: vec!["book.epub".into()],
                source: ImportSource::new(Box::new(move || {
                    Box::pin(async move {
                        assert!(!destination.join("broken.epub").exists(), "copy the whole selection before publication or inspection");
                        Ok(Box::new(Cursor::new(bytes)) as DirectoryImportReader)
                    })
                })),
            },
            DirectoryImportFile { path: vec!["interrupted.epub".into()], source: ImportSource::new(Box::new(|| Box::pin(async { Ok(Box::new(InterruptedReader(false)) as DirectoryImportReader) }))) },
        ],
    };
    let failures = service.import(&*service.state.write().await).import_selected_directory(shelf.id, directory, false).await.unwrap();
    assert_eq!(failures.len(), 2);
    assert_eq!(std::fs::read(library.path().join("Shelf/broken.epub")).unwrap(), b"invalid EPUB");
    assert_eq!(std::fs::read(library.path().join("Shelf/book.epub")).unwrap(), bytes);
    assert!(!library.path().join("Shelf/interrupted.epub").exists());
    let hash = service.state.read().await.database().book_for_local_content(blake3::hash(bytes).into()).await.unwrap().unwrap();
    let paths = service.state.read().await.database().book_paths(&hash).await.unwrap();
    assert!(!paths.is_empty());
    assert!(paths.iter().all(|path| path.as_str() == "/Shelf/book.epub"));
    assert!(std::fs::read_dir(library.path().join("Shelf")).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(".bokheim-copy-")));
}

#[tokio::test]
async fn selected_folder_import_survives_source_removal_and_library_reopen() {
    use crate::{DirectoryImport, DirectoryImportFile, DirectoryImportReader, ImportSource};
    use std::fs::{self, File};

    let library = tempfile::tempdir().unwrap();
    let provider = tempfile::tempdir().unwrap();
    let source = provider.path().join("book.epub");
    let bytes = include_bytes!("../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
    fs::write(&source, bytes).unwrap();
    let hash;
    {
        let (service, _handle) = fixture(library.path(), "http://127.0.0.1:1".parse().unwrap()).await;
        let directory = DirectoryImport {
            name: "Selected books".into(),
            directories: vec![vec!["Nested".into()]],
            files: vec![DirectoryImportFile {
                path: vec!["Nested".into(), "book.epub".into()],
                source: ImportSource::new(Box::new(move || Box::pin(async move { Ok(Box::new(File::open(source).map_err(|error| error.to_string())?) as DirectoryImportReader) }))),
            }],
        };
        let failures = service.import(&*service.state.write().await).import_selected_directory(sync_common::ROOT_DIR_ID, directory, false).await.unwrap();
        assert!(failures.is_empty(), "{failures:?}");
        hash = service.state.read().await.database().book_for_local_content(blake3::hash(bytes).into()).await.unwrap().unwrap();
        assert_eq!(fs::read(provider.path().join("book.epub")).unwrap(), bytes, "importing must leave the selected source untouched");
    }
    // A completed picker import must need neither the provider nor its URI
    // grant after restart. Only the managed library is opened again.
    provider.close().unwrap();
    let (service, _handle) = fixture(library.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    assert_eq!(fs::read(library.path().join("Nested/book.epub")).unwrap(), bytes);
    let paths = service.state.read().await.database().book_paths(&hash).await.unwrap();
    assert!(!paths.is_empty());
    assert!(paths.iter().all(|path| path.as_str() == "/Nested/book.epub"));
}

#[tokio::test]
async fn selected_folder_import_uses_shared_folder_collision_handling() {
    use crate::{DirectoryImport, DirectoryImportFile, DirectoryImportReader, ImportSource};
    use std::io::Cursor;

    let library = tempfile::tempdir().unwrap();
    let (service, _handle) = fixture(library.path(), "http://127.0.0.1:1".parse().unwrap()).await;
    let shelf = service.state.write().await.create_directory(&sync_common::ROOT_DIR_ID, "Shelf").await.unwrap();
    let existing = service.state.write().await.create_directory(&shelf.id, "Books").await.unwrap();
    let bytes = include_bytes!("../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
    let directory = DirectoryImport {
        name: "Books".into(),
        directories: vec![vec!["Nested".into()], vec!["Empty".into()]],
        files: vec![DirectoryImportFile { path: vec!["Nested".into(), "book.epub".into()], source: ImportSource::new(Box::new(move || Box::pin(async move { Ok(Box::new(Cursor::new(bytes)) as DirectoryImportReader) }))) }],
    };
    let failures = service.import(&*service.state.write().await).import_selected_directory(shelf.id, directory, true).await.unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    assert!(library.path().join("Shelf/Books 2/Empty").is_dir());
    assert_eq!(std::fs::read(library.path().join("Shelf/Books 2/Nested/book.epub")).unwrap(), bytes);
    assert_eq!(service.state.read().await.database().child_directory(&shelf.id, "Books").await.unwrap(), Some(existing.id));
    let hash = service.state.read().await.database().book_for_local_content(blake3::hash(bytes).into()).await.unwrap().unwrap();
    let paths = service.state.read().await.database().book_paths(&hash).await.unwrap();
    assert!(!paths.is_empty());
    assert!(paths.iter().all(|path| path.as_str() == "/Shelf/Books 2/Nested/book.epub"));
}
