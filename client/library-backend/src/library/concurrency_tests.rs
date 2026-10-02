use super::*;
use crate::library::{PendingCommand, ReadyCommand, RuntimeCommand, SessionCommand};
use crate::test_database::raw;
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

fn fixture(root: &std::path::Path, url: account_client::ServerUrl) -> (LibraryActor, LibraryHandle, LibraryEventReceiver, LibraryEventSender, async_channel::Receiver<LibraryActorCommand>) {
    let account = library_runtime::account::LibraryAccountSession::load(Account(account_client::Session::try_new(url.clone(), "user".into(), "reader@example.com".into(), "a".repeat(43)).unwrap()), url.clone()).unwrap();
    let mut session = LibrarySession::open(super::super::LibraryRuntimeSpec {
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
        remote_changes: Default::default(),
    })
    .unwrap();
    let events = session.event_rx.take().unwrap();
    let event_tx = session.event_tx.clone();
    let (commands, inbox) = async_channel::unbounded();
    let handle = LibraryHandle { id: session.id, commands, subscriptions: session.subscriptions.clone(), runtime: session.runtime.clone(), event_tx: session.event_tx.clone(), sync_wake: session.sync_wake.clone() };
    let actor = LibraryActor::new(session);
    (actor, handle, events, event_tx, inbox)
}

fn enqueue(handle: &LibraryHandle, command: SessionCommand) -> async_channel::Receiver<Result<client_runtime::reply::Reply, BackendError>> {
    let (reply, receiver) = async_channel::bounded(1);
    handle.commands.try_send(LibraryActorCommand::Ui { command, reply }).unwrap_or_else(|_| panic!("actor open"));
    receiver
}

#[tokio::test]
async fn folder_replay_runs_while_enrichment_is_stalled() {
    let root = tempfile::tempdir().unwrap();
    let (mut actor, handle, events, _, inbox) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    actor.session.filesystem_scanning.set(false);
    let folder = actor.session.create_directory(&sync_common::ROOT_DIR_ID, "Before").unwrap();
    crate::asset_workflow::prepare_directory(&actor.session.assets, &actor.session.db, folder.id).await.unwrap();
    assert!(root.path().join("Before").is_dir());
    // A held enrichment batch must not be replaced, completed, or awaited
    // before either the first replay or a subsequent wake can make progress.
    actor.enrichment_job.task = Some(Box::pin(std::future::pending()));
    let event_commands = handle.commands.clone();
    let exercise = async {
        for name in ["After", "Again"] {
            handle.dispatch(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::RenameDirectory { directory_id: folder.id, name: name.into() }))).await.unwrap();
            while !root.path().join(name).is_dir() {
                tokio::task::yield_now().await;
            }
        }
        assert!(!root.path().join("Before").exists());
        assert!(!root.path().join("After").exists());
        handle.shutdown();
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(actor.run(inbox, events, event_commands), exercise);
    })
    .await
    .expect("physical renames must not wait for enrichment");
}

#[tokio::test]
async fn retiring_before_first_poll_discards_queued_commands_and_replies() {
    let root = tempfile::tempdir().unwrap();
    let (actor, handle, events, _, inbox) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    let response = enqueue(&handle, SessionCommand::Ready(ReadyCommand::CreateDirectory { parent_id: sync_common::ROOT_DIR_ID, name: "Must not run".into() }));
    let database = actor.session.db.clone();
    handle.shutdown();
    actor.run(inbox, events, handle.commands.clone()).await;
    assert!(response.recv().await.is_err());
    assert_eq!(raw(&database).query_row("SELECT count(*) FROM dir WHERE name='Must not run'", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
}

#[tokio::test]
async fn unpolled_staged_preparation_does_not_leak_import_activity() {
    let root = tempfile::tempdir().unwrap();
    let (actor, _, _, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    let work = super::super::commands::prepare_library_request(
        actor.session.clone(),
        SessionCommand::Pending(PendingCommand::PrepareStagedDirectoryImport {
            activity: uuid::Uuid::new_v4(),
            job: uuid::Uuid::new_v4().to_string(),
            parent_id: sync_common::ROOT_DIR_ID,
            name: Some("Unstarted".into()),
            directories: vec![],
        }),
    )
    .unwrap();
    assert!(actor.session.runtime.has_directory_imports());
    drop(work);
    assert!(!actor.session.runtime.has_directory_imports());
}

#[tokio::test]
async fn staged_preparation_hands_off_only_after_success() {
    let root = tempfile::tempdir().unwrap();
    let (actor, _, _, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    for valid in [false, true] {
        let activity = uuid::Uuid::new_v4();
        let work = super::super::commands::prepare_library_request(
            actor.session.clone(),
            SessionCommand::Pending(PendingCommand::PrepareStagedDirectoryImport {
                activity,
                job: if valid { uuid::Uuid::new_v4().to_string() } else { "invalid journal".into() },
                parent_id: sync_common::ROOT_DIR_ID,
                name: None,
                directories: vec![],
            }),
        )
        .unwrap();
        let PreparedRequest::Pending(operation) = work else { panic!("preparation must run beside commands") };
        assert_eq!(operation.future.await.is_ok(), valid);
        assert_eq!(actor.session.runtime.has_directory_imports(), valid);
        actor.session.finish_directory_import(activity);
        assert!(!actor.session.runtime.has_directory_imports());
    }
}

#[tokio::test]
async fn create_folder_materializes_before_returning_without_queued_work() {
    let root = tempfile::tempdir().unwrap();
    let (actor, _, _, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    std::fs::create_dir(root.path().join("Shelf")).unwrap();
    std::fs::write(root.path().join("Shelf/keep.txt"), "existing data").unwrap();
    let folder = actor.session.create_directory(&sync_common::ROOT_DIR_ID, "Shelf").unwrap();
    let relative = actor.session.db.directory_relative_path_string(&folder.id).unwrap().unwrap();
    assert_ne!(relative, "Shelf");
    assert_eq!(std::fs::read_to_string(root.path().join(&relative).join(".biblos_uuid")).unwrap(), folder.id.to_string());
    let child = actor.session.create_directory(&folder.id, "Child").unwrap();
    assert_eq!(std::fs::read_to_string(root.path().join(&relative).join("Child/.biblos_uuid")).unwrap(), child.id.to_string());
    assert!(!actor.session.db.has_pending_file_work().unwrap());
    assert_eq!(std::fs::read_to_string(root.path().join("Shelf/keep.txt")).unwrap(), "existing data");
    // An unavailable parent must fail before creating a database record.
    std::fs::rename(root.path().join(&relative), root.path().join("Moved externally")).unwrap();
    assert!(actor.session.create_directory(&folder.id, "Cannot create").is_err());
    assert_eq!(raw(&actor.session.db).query_row("SELECT count(*) FROM dir WHERE name='Cannot create'", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert!(!actor.session.db.has_pending_file_work().unwrap());
}

#[cfg(feature = "scanner")]
#[tokio::test]
async fn scan_recovers_folder_created_before_database_registration() {
    let root = tempfile::tempdir().unwrap();
    let (mut actor, _, _, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    let id = DirId::new_v4();
    actor.session.assets.create_directory("Recovered", id).unwrap();
    assert!(actor.session.db.directory_relative_path_string(&id).unwrap().is_none());
    actor.begin_scan();
    let result = actor.scan_job.task.as_mut().unwrap().await;
    actor.finish_scan(result);
    assert_eq!(actor.session.db.directory_relative_path_string(&id).unwrap().as_deref(), Some("Recovered"));
    assert!(!actor.session.db.has_pending_file_work().unwrap());
}

#[cfg(feature = "scanner")]
#[tokio::test]
async fn invalid_book_finishes_scan_with_a_file_error() {
    let root = tempfile::tempdir().unwrap();
    let (mut actor, _, _, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    std::fs::write(root.path().join("Broken.epub"), b"not a zip archive").unwrap();
    actor.begin_scan();
    let result = actor.scan_job.task.as_mut().unwrap().await;
    actor.finish_scan(result);
    assert!(!actor.session.scanner_running());
    assert!(!actor.session.db.needs_scan().unwrap());
    let failures = actor.session.db.scan_failures().unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, "Broken.epub");
    assert!(!failures[0].1.is_empty());
}

#[tokio::test]
async fn queued_download_is_durable_and_repeated_requests_share_one_transfer() {
    let root = tempfile::tempdir().unwrap();
    let (actor, _, _, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    let hash = ContentHash::new(&"e".repeat(64));
    raw(&actor.session.db).execute("INSERT INTO book(content_hash,title,format) VALUES(?1,'Remote',1)", [hash.as_str()]).unwrap();
    raw(&actor.session.db).execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT ?1,row_id,'remote.epub','',0 FROM book", [sync_common::ROOT_DIR_ID.to_string()]).unwrap();
    assert_eq!(actor.session.queue_download(hash).unwrap(), DownloadState::Queued);
    assert!(actor.operations.is_empty());
    assert!(actor.session.db.is_book_download_requested(&hash).unwrap());
    assert_eq!(actor.session.sync.transfer_queue.statuses()[0].total_items, 1);
    assert_eq!(actor.session.queue_download(hash).unwrap(), DownloadState::Queued);
    assert_eq!(actor.session.sync.transfer_queue.statuses()[0].total_items, 1, "repeated requests share one queued transfer");
}

#[tokio::test]
async fn stalled_enrichment_does_not_hold_new_upload_admission() {
    let root = tempfile::tempdir().unwrap();
    let (mut actor, _, _, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    actor.enrichment_job.task = Some(Box::pin(std::future::pending()));
    let hash = ContentHash::new(&"f".repeat(64));
    raw(&actor.session.db).execute("INSERT INTO book(content_hash,title,format) VALUES(?1,'New book',1)", [hash.as_str()]).unwrap();
    actor.admit_and_enrich();
    assert_eq!(raw(&actor.session.db).query_row("SELECT COUNT(*) FROM book_transfer WHERE content_hash=?1 AND upload_prepared IS NOT NULL", [hash.as_str()], |row| row.get::<_, i64>(0)).unwrap(), 1);
    assert!(actor.enrichment_job.task.is_some());
    assert!(actor.enrichment_job.pending);
}

#[tokio::test]
async fn import_progress_bypasses_unpolled_actor_and_rejects_retired_handle() {
    let root = tempfile::tempdir().unwrap();
    let (actor, handle, _events, _, inbox) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    let activity = uuid::Uuid::new_v4();
    actor.session.begin_directory_import(activity);
    tokio::time::timeout(Duration::from_secs(1), async {
        handle.dispatch(LibraryCommand::Runtime(RuntimeCommand::AdvanceDirectoryImport { activity, total: 3, succeeded: 1, failed: 1 })).await.unwrap();
        let progress = handle.dispatch(LibraryCommand::Runtime(RuntimeCommand::ImportProgress { activity })).await.unwrap().take::<Option<library_model::DirectoryImportProgress>>().unwrap().unwrap();
        assert_eq!((progress.total, progress.succeeded, progress.failed), (3, 1, 1));
        handle.dispatch(LibraryCommand::Runtime(RuntimeCommand::FinishDirectoryImport { activity })).await.unwrap();
        assert!(handle.dispatch(LibraryCommand::Runtime(RuntimeCommand::ImportProgress { activity })).await.unwrap().take::<Option<library_model::DirectoryImportProgress>>().unwrap().is_none());
    })
    .await
    .expect("shared progress must not need the actor to be polled");
    assert!(inbox.is_empty());
    handle.shutdown();
    assert!(handle.dispatch(LibraryCommand::Runtime(RuntimeCommand::DirectoryImportProgress)).await.is_err());
}

#[tokio::test]
async fn stored_import_rejects_changed_length_without_consuming_source() {
    use crate::library::commands::dispatch_library_request_in_session;
    use crate::library::{ImportBookSource, StagedImportMetadata};
    let root = tempfile::tempdir().unwrap();
    let (actor, _, _, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
    let staged = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(staged.path(), b"changed bytes").unwrap();
    let command = LibraryCommand::Session(SessionCommand::Pending(PendingCommand::ImportBook {
        parent_id: sync_common::ROOT_DIR_ID,
        file_name: "book.epub".into(),
        source: ImportBookSource::Stored { location: staged.path().to_owned(), staged: Some(StagedImportMetadata { length: 1, hash: ContentHash::new(&"a".repeat(64)) }) },
    }));
    let result = dispatch_library_request_in_session(actor.session.clone(), command).await;
    assert!(matches!(result, Err(error) if error.to_string().contains("staged import length changed")));
    assert_eq!(std::fs::read(staged.path()).unwrap(), b"changed bytes");
    assert_eq!(actor.session.book_count().unwrap(), 0);
}
