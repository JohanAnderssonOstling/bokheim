use super::*;
use crate::library::{LibraryDirectory, LibraryNotifications, ReadyCommand, LibraryCommand};
use library_runtime::interest::{InterestState, NotificationInterest};
use std::sync::Arc;
use sync_common::api::notifications::NotificationMessage;
use web_time::Instant;
use std::time::Duration;

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

fn fixture(root: &std::path::Path, url: account_client::Url) -> (LibraryActor, LibraryHandle, async_channel::Receiver<LibraryActorCommand>) {
    let account = library_runtime::account::LibraryAccountSession::load(Account(account_client::Session::try_new(url.clone(), "user".into(), "reader@example.com".into(), "a".repeat(43)).unwrap()), url.clone()).unwrap();
    let session = LibrarySession::open(super::super::LibraryRuntimeSpec {
        config: super::super::LibraryOpenConfig {
            id: LibraryId::new_v4(),
            display_name: "Concurrency".into(),
            root: root.to_string_lossy().into_owned(),
            database_locator: root.join("library.sqlite").to_string_lossy().into_owned(),
            default_sync_server_url: url,
            metadata_server_url: "http://127.0.0.1:1".into(),
            asset_storage_enabled: true,
            reset_cloud_presence: false,
        },
        account: account.library_account(),
        notification_interest: Default::default(),
    })
    .unwrap();
    let (commands, inbox) = async_channel::unbounded();
    let handle = LibraryHandle { id: session.id, commands };
    let actor = LibraryActor::new(session);
    (actor, handle, inbox)
}

#[tokio::test]
async fn remote_notifications_route_to_the_requested_library_and_reconnect_refreshes_all() {
    use library_runtime::scheduler::SyncRequest;

    let first_root = tempfile::tempdir().unwrap();
    let second_root = tempfile::tempdir().unwrap();
    let (mut first_actor, first, first_inbox) = fixture(first_root.path(), "http://127.0.0.1:1".parse().unwrap());
    let (mut second_actor, second, second_inbox) = fixture(second_root.path(), "http://127.0.0.1:1".parse().unwrap());
    let directory = Arc::new(LibraryDirectory::default());
    directory.insert(first.clone()).unwrap();
    directory.insert(second.clone()).unwrap();
    let notifications = LibraryNotifications::new(directory.clone(), first_actor.session.account.clone(), InterestState::default());

    notifications.receive(&NotificationMessage::LibraryChanged { library_id: first.id() }, true).unwrap();
    first_actor.handle_command(first_inbox.try_recv().unwrap());
    assert!(matches!(first_actor.requests.as_ref().unwrap().sync.try_recv(), Ok(SyncRequest::RefreshRemote)));
    assert!(second_inbox.is_empty(), "a library notification must not wake other libraries");
    assert!(second_actor.requests.as_ref().unwrap().sync.is_empty());

    let hello = NotificationMessage::Hello { heartbeat_seconds: 5, timeout_seconds: 15 };
    notifications.receive(&hello, false).unwrap();
    assert!(first_inbox.is_empty(), "reader-only connections must not reconcile on Hello");
    assert!(second_inbox.is_empty());
    for message in [hello, NotificationMessage::LibrariesChanged, NotificationMessage::Reconcile] {
        notifications.receive(&message, true).unwrap();
        first_actor.handle_command(first_inbox.try_recv().unwrap());
        second_actor.handle_command(second_inbox.try_recv().unwrap());
        assert!(matches!(first_actor.requests.as_ref().unwrap().sync.try_recv(), Ok(SyncRequest::RefreshRemote)));
        assert!(matches!(second_actor.requests.as_ref().unwrap().sync.try_recv(), Ok(SyncRequest::RefreshRemote)));
    }

    notifications.receive(&NotificationMessage::LibraryChanged { library_id: LibraryId::new_v4() }, true).unwrap();
    assert!(first_inbox.is_empty());
    assert!(second_inbox.is_empty());

    directory.retire(&first.id()).unwrap();
    notifications.receive(&NotificationMessage::LibraryChanged { library_id: first.id() }, true).unwrap();
    notifications.receive(&NotificationMessage::Reconcile, true).unwrap();
    assert!(directory.try_get(&first.id()).unwrap().is_none());
    assert!(first_actor.requests.as_ref().unwrap().sync.is_empty());
    second_actor.handle_command(second_inbox.try_recv().unwrap());
    assert!(matches!(second_actor.requests.as_ref().unwrap().sync.try_recv(), Ok(SyncRequest::RefreshRemote)));
}

#[tokio::test]
async fn foreground_resume_reconciles_libraries_without_a_socket_and_does_not_poll() {
    use library_runtime::scheduler::SyncRequest;

    let first_root = tempfile::tempdir().unwrap();
    let second_root = tempfile::tempdir().unwrap();
    let (mut first_actor, first, first_inbox) = fixture(first_root.path(), "http://127.0.0.1:1".parse().unwrap());
    let (mut second_actor, second, second_inbox) = fixture(second_root.path(), "http://127.0.0.1:1".parse().unwrap());
    let account = first_actor.session.account.clone();
    let directory = Arc::new(LibraryDirectory::default());
    directory.insert(first).unwrap();
    directory.insert(second).unwrap();
    let interest = NotificationInterest::default();
    let activity = interest.subscribe();
    let mut notifications = LibraryNotifications::new(directory, account.clone(), *activity.borrow());
    let now = Instant::now();
    let view = interest.view(false);
    assert!(!activity.borrow().enabled(), "reader-only foreground activity needs no socket");
    assert!(!notifications.observe_activity_at(*activity.borrow(), now + Duration::from_secs(1)));
    assert!(first_inbox.is_empty());
    assert!(second_inbox.is_empty());

    // A deactivate/reactivate pair can coalesce into one activity update.
    drop(view);
    let view = interest.view(false);
    assert!(notifications.observe_activity_at(*activity.borrow(), now + Duration::from_secs(31)));
    first_actor.handle_command(first_inbox.try_recv().unwrap());
    second_actor.handle_command(second_inbox.try_recv().unwrap());
    assert!(matches!(first_actor.requests.as_ref().unwrap().sync.try_recv(), Ok(SyncRequest::RefreshRemote)));
    assert!(matches!(second_actor.requests.as_ref().unwrap().sync.try_recv(), Ok(SyncRequest::RefreshRemote)));
    assert!(!notifications.observe_activity_at(*activity.borrow(), now + Duration::from_secs(40)));

    drop(view);
    let view = interest.view(false);
    assert!(!notifications.observe_activity_at(*activity.borrow(), now + Duration::from_secs(45)), "rapid resumes are throttled");
    assert!(first_inbox.is_empty());
    assert!(second_inbox.is_empty());
    drop(view);
    let view = interest.view(false);
    assert!(notifications.observe_activity_at(*activity.borrow(), now + Duration::from_secs(65)));
    first_actor.handle_command(first_inbox.try_recv().unwrap());
    second_actor.handle_command(second_inbox.try_recv().unwrap());
    assert!(matches!(first_actor.requests.as_ref().unwrap().sync.try_recv(), Ok(SyncRequest::RefreshRemote)));
    assert!(matches!(second_actor.requests.as_ref().unwrap().sync.try_recv(), Ok(SyncRequest::RefreshRemote)));

    let token = account.require().unwrap().token().to_owned();
    assert!(account.refresh_after_unauthorized(token).await.unwrap().is_none());
    drop(view);
    let _view = interest.view(false);
    assert!(!notifications.observe_activity_at(*activity.borrow(), now + Duration::from_secs(100)), "signed-out resumes do not trigger discovery or library refresh");
    assert!(first_inbox.is_empty());
    assert!(second_inbox.is_empty());
}

#[tokio::test]
async fn subscription_registers_with_initial_state_and_receives_callback_events() {
    let root = tempfile::tempdir().unwrap();
    let (mut actor, handle, inbox) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    actor.session.begin_directory_import(uuid::Uuid::from_u128(1));
    let events = actor.session.events.clone();
    let hash = ContentHash::from_bytes([3; 32]);
    let exercise = async {
        let (updates, scanning) = handle.subscribe_updates().await.unwrap();
        assert!(scanning, "the subscription snapshot includes the active import");
        // Callback capabilities can cross threads without sharing actor state.
        std::thread::spawn(move || events.emit(LibraryEvent::ThumbnailGenerated { content_hash: hash })).join().unwrap();
        assert_eq!(updates.recv().await.unwrap(), library_model::LibraryUpdate::Covers(vec![hash]));
        handle.shutdown();
        assert!(updates.recv().await.is_err());
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(actor.serve(inbox), exercise);
    }).await.expect("subscription and event delivery must make progress");
}

#[tokio::test]
async fn retirement_stops_buffered_updates_without_polling_the_actor() {
    use crate::library::subscriptions::LibraryUpdates;
    let root = tempfile::tempdir().unwrap();
    let (mut actor, handle, _inbox) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    let (reply, result) = async_channel::bounded(1);
    actor.handle_command(LibraryActorCommand::SubscribeUpdates { reply });
    let (updates, _) = result.try_recv().unwrap().unwrap();
    let updates = LibraryUpdates::new(updates, handle.commands.clone());
    actor.session.emit(LibraryEvent::ContentsChanged);
    handle.shutdown();
    assert_eq!(updates.try_recv(), Err(async_channel::TryRecvError::Closed));
    assert!(updates.recv().await.is_err());
    assert!(handle.subscribe_updates().await.is_err());
}

#[tokio::test]
async fn retirement_cancels_a_subscription_still_waiting_in_the_inbox() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let root = tempfile::tempdir().unwrap();
    let (_actor, handle, inbox) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    let mut subscribing = std::pin::pin!(handle.subscribe_updates());
    assert!(matches!(subscribing.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Pending));
    assert_eq!(inbox.len(), 1);
    handle.shutdown();
    assert!(tokio::time::timeout(Duration::from_secs(1), subscribing).await.unwrap().is_err());
}

#[tokio::test]
async fn mutations_publish_queued_events_before_acknowledgement() {
    let root = tempfile::tempdir().unwrap();
    let (mut actor, _handle, _inbox) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    let (reply, result) = async_channel::bounded(1);
    actor.handle_command(LibraryActorCommand::SubscribeUpdates { reply });
    let (updates, _) = result.try_recv().unwrap().unwrap();

    let (reply, result) = async_channel::bounded(1);
    actor.handle_command(LibraryActorCommand::Ui {
        command: LibraryCommand::Ready(ReadyCommand::CreateDirectory { parent_id: sync_common::ROOT_DIR_ID, name: "Shelf".into() }),
        reply,
    });
    assert!(result.try_recv().unwrap().is_ok());
    assert_eq!(updates.try_recv().unwrap(), library_model::LibraryUpdate::Contents);

    let events = actor.session.events.clone();
    let (reply, result) = async_channel::bounded(1);
    actor.submit(OperationKind::Read, PendingReply::new(reply), async move {
        events.emit(LibraryEvent::ContentsChanged);
        Ok(())
    });
    actor.operations.next().await.unwrap();
    assert!(result.try_recv().unwrap().is_ok());
    assert_eq!(updates.try_recv().unwrap(), library_model::LibraryUpdate::Contents);
}

#[tokio::test]
async fn folder_replay_follows_renames_while_the_actor_serves() {
    let root = tempfile::tempdir().unwrap();
    let (actor, handle, inbox) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    actor.session.filesystem_scanning.set(false);
    let folder = actor.session.placement.create_directory(&sync_common::ROOT_DIR_ID, "Before").unwrap();
    library_placement::prepare_directory(&actor.session.assets, &actor.session.db, folder.id).await.unwrap();
    assert!(root.path().join("Before").is_dir());
    let exercise = async {
        for name in ["After", "Again"] {
            handle.dispatch(LibraryCommand::Ready(ReadyCommand::RenameDirectory { directory_id: folder.id, name: name.into() })).await.unwrap();
            while !root.path().join(name).is_dir() {
                tokio::task::yield_now().await;
            }
        }
        assert!(!root.path().join("Before").exists());
        assert!(!root.path().join("After").exists());
        handle.shutdown();
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(actor.run(inbox, async {}), exercise);
    })
    .await
    .expect("each rename reaches the disk");
}

#[tokio::test]
async fn external_import_leaves_failed_imports_on_disk_for_retry() {
    for moving in [false, true] {
    let library = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let source = external.path().join("broken.epub");
    std::fs::write(&source, b"invalid EPUB").unwrap();
    let (actor, _, _) = fixture(library.path(), "http://127.0.0.1:1".parse().unwrap());
    let shelf = actor.session.placement.create_directory(&sync_common::ROOT_DIR_ID, "Shelf").unwrap();
    let failures = actor.session.import_paths(shelf.id, vec![source.clone()], moving).await.unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(source.exists(), !moving);
    let moved = library.path().join("Shelf/broken.epub");
    assert_eq!(std::fs::read(&moved).unwrap(), b"invalid EPUB");
    let failures = actor.session.import_paths(shelf.id, vec![moved.clone()], true).await.unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(std::fs::read(&moved).unwrap(), b"invalid EPUB");
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
    let (actor, _, _) = fixture(library.path(), "http://127.0.0.1:1".parse().unwrap());
    let shelf = actor.session.placement.create_directory(&sync_common::ROOT_DIR_ID, "Shelf").unwrap();
    let failures = actor.session.import_paths(shelf.id, vec![source.clone()], moving).await.unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(source.exists(), !moving);
    assert_eq!(std::fs::read(library.path().join("Shelf/Books/Nested/book.epub")).unwrap(), bytes);
    assert_eq!(std::fs::read(library.path().join("Shelf/Books/notes.txt")).unwrap(), b"sidecar");
    let hash = ContentHash::from(blake3::hash(bytes));
    let mut paths: Vec<_> = actor.session.db.book_paths(&hash).unwrap().iter().map(|path| path.to_string()).collect();
    paths.sort();
    paths.dedup();
    assert_eq!(paths, vec!["/Shelf/Books/Nested/book.epub"]);
    let failures = actor.session.import_paths(shelf.id, vec![library.path().join("Shelf/Books")], true).await.unwrap();
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
            if self.0 { return Err(std::io::Error::other("copy interrupted")); }
            self.0 = true;
            let count = bytes.len().min(4);
            bytes[..count].copy_from_slice(&b"data"[..count]);
            Ok(count)
        }
    }

    let library = tempfile::tempdir().unwrap();
    let (actor, _, _) = fixture(library.path(), "http://127.0.0.1:1".parse().unwrap());
    let shelf = actor.session.placement.create_directory(&sync_common::ROOT_DIR_ID, "Shelf").unwrap();
    let bytes = include_bytes!("../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
    let destination = library.path().join("Shelf");
    let directory = DirectoryImport {
        name: "Selected books".into(),
        directories: vec![],
        files: vec![
            DirectoryImportFile {
                path: vec!["broken.epub".into()],
                source: ImportSource::new(Box::new(|| Box::pin(async { Ok(Box::new(Cursor::new(b"invalid EPUB")) as DirectoryImportReader) }))),
            },
            DirectoryImportFile {
                path: vec!["book.epub".into()],
                source: ImportSource::new(Box::new(move || Box::pin(async move {
                    assert!(!destination.join("broken.epub").exists(), "copy the whole selection before publication or inspection");
                    Ok(Box::new(Cursor::new(bytes)) as DirectoryImportReader)
                }))),
            },
            DirectoryImportFile {
                path: vec!["interrupted.epub".into()],
                source: ImportSource::new(Box::new(|| Box::pin(async { Ok(Box::new(InterruptedReader(false)) as DirectoryImportReader) }))),
            },
        ],
    };
    let failures = actor.session.import_selected_directory(shelf.id, directory, false, &|| false).await.unwrap();
    assert_eq!(failures.len(), 2);
    assert_eq!(std::fs::read(library.path().join("Shelf/broken.epub")).unwrap(), b"invalid EPUB");
    assert_eq!(std::fs::read(library.path().join("Shelf/book.epub")).unwrap(), bytes);
    assert!(!library.path().join("Shelf/interrupted.epub").exists());
    let hash = ContentHash::from(blake3::hash(bytes));
    let paths = actor.session.db.book_paths(&hash).unwrap();
    assert!(!paths.is_empty());
    assert!(paths.iter().all(|path| path.as_str() == "/Shelf/book.epub"));
    assert!(std::fs::read_dir(library.path().join("Shelf")).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(".bokheim-copy-")));
}

#[tokio::test]
async fn selected_folder_import_uses_shared_folder_collision_handling() {
    use crate::{DirectoryImport, DirectoryImportFile, DirectoryImportReader, ImportSource};
    use std::io::Cursor;

    let library = tempfile::tempdir().unwrap();
    let (actor, _, _) = fixture(library.path(), "http://127.0.0.1:1".parse().unwrap());
    let shelf = actor.session.placement.create_directory(&sync_common::ROOT_DIR_ID, "Shelf").unwrap();
    let existing = actor.session.placement.create_directory(&shelf.id, "Books").unwrap();
    let bytes = include_bytes!("../../crates/book-inspection/tests/fixtures/back-matter-evidence.epub");
    let directory = DirectoryImport {
        name: "Books".into(),
        directories: vec![vec!["Nested".into()], vec!["Empty".into()]],
        files: vec![DirectoryImportFile {
            path: vec!["Nested".into(), "book.epub".into()],
            source: ImportSource::new(Box::new(move || Box::pin(async move { Ok(Box::new(Cursor::new(bytes)) as DirectoryImportReader) }))),
        }],
    };
    let failures = actor.session.import_selected_directory(shelf.id, directory, true, &|| false).await.unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    assert!(library.path().join("Shelf/Books 2/Empty").is_dir());
    assert_eq!(std::fs::read(library.path().join("Shelf/Books 2/Nested/book.epub")).unwrap(), bytes);
    assert_eq!(actor.session.db.child_directory(&shelf.id, "Books").unwrap(), Some(existing.id));
    let hash = ContentHash::from(blake3::hash(bytes));
    let paths = actor.session.db.book_paths(&hash).unwrap();
    assert!(!paths.is_empty());
    assert!(paths.iter().all(|path| path.as_str() == "/Shelf/Books 2/Nested/book.epub"));
}
