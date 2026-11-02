//! Copy into a separate library; provider files and the source stay untouched.
use super::{LibraryEntry, core::AppBackend};
use crate::{BackendError, Failure};
use library_backend::library::StorageCopyProgress;
use library_backend::library::handle::{LibraryActorCommand, LibraryControl};
use library_model::CloudQuotaBook;
use library_backend::{LibraryCommand, PendingCommand, ReadyCommand};
use serde::{Deserialize, Serialize};
use sync_common::{LibraryId, api::assets::BlobManifestEntry, storage::LibraryStorage};

#[derive(Clone, Serialize, Deserialize)]
struct CopyJournal {
    destination: LibraryId,
    progress: StorageCopyProgress,
}

impl AppBackend {
    pub(super) fn cloud_copy_remaining_bytes(&self, source: LibraryId, books: &[CloudQuotaBook]) -> Result<Option<u64>, BackendError> {
        let journal: Option<CopyJournal> = self.state.registry.read_value(&format!("storage-copy:v1:{source}:1")).map_err(BackendError::operation)?.map(|bytes| serde_json::from_slice(&bytes).map_err(BackendError::operation)).transpose()?;
        Ok(books.iter().filter(|source_book| !journal.as_ref().is_some_and(|journal| journal.progress.books.iter().any(|book| book.source == source_book.book_id))).try_fold(0_u64, |bytes, book| bytes.checked_add(book.size_bytes)))
    }

    pub(crate) async fn copy_library_storage(&self, source: LibraryId, bokheim: bool) -> Result<LibraryEntry, BackendError> {
        // Requests from different windows must resume the same pending copy.
        let _copy = self.state.storage_copy_lock.lock().await;
        self.copy_library_storage_inner(source, bokheim, false).await
    }

    async fn copy_library_storage_inner(&self, source: LibraryId, bokheim: bool, keep_journal: bool) -> Result<LibraryEntry, BackendError> {
        self.library_entry(&source)?;
        if bokheim && !self.account_status()?.signed_in() {
            return Err(BackendError::classified(Failure::SignInRequired, "sign in before copying a library to Bokheim storage"));
        }
        let key = format!("storage-copy:v1:{source}:{}", u8::from(bokheim));
        if bokheim && self.state.registry.read_value(&key).map_err(BackendError::operation)?.is_none() {
            let library = self.state.library_directory.get(&source)?;
            let books: Vec<CloudQuotaBook> = library.dispatch(LibraryCommand::Ready(ReadyCommand::CloudQuotaBooks)).await?.take()?;
            let _quota = self.admit_cloud_books(books.into_iter().map(|book| BlobManifestEntry { byte_checksum: book.byte_checksum, content_hash: book.book_id, size_bytes: book.size_bytes }).collect()).await?;
        }
        let journal_key = key.clone();
        let (entry, mut journal) = self.with_storage(move |this| {
            let original = this.library_entry(&source)?;
            let journal: Option<CopyJournal> = this.state.registry.read_value(&journal_key).map_err(BackendError::operation)?.map(|bytes| serde_json::from_slice(&bytes).map_err(BackendError::operation)).transpose()?;
            if let Some(journal) = journal {
                if let Some(entry) = this.state.registry.entry(&journal.destination).map_err(BackendError::operation)? {
                    let expected = if bokheim { LibraryStorage::Bokheim } else { LibraryStorage::Local };
                    if this.state.registry.storage(&journal.destination).map_err(BackendError::operation)? != expected {
                        return Err(BackendError::message("the copy's storage has changed; restore its storage setting before retrying"));
                    }
                    if !bokheim {
                        this.state.registry.make_device_only(entry.library_id()).map_err(BackendError::operation)?;
                    }
                    return Ok((entry, journal));
                }
            }
            let name = format!("{} ({})", original.library_name(), if bokheim { "Bokheim copy" } else { "local copy" });
            let entry = this.state.registry.create_with_asset_storage(&name, None, bokheim).map_err(BackendError::operation)?;
            if bokheim {
                this.bind_library_to_current_account(entry.library_id())?;
            } else {
                this.state.registry.make_device_only(entry.library_id()).map_err(BackendError::operation)?;
            }
            let journal = CopyJournal { destination: *entry.library_id(), progress: Default::default() };
            this.state.registry.write_value(&journal_key, &serde_json::to_vec(&journal).map_err(BackendError::operation)?).map_err(BackendError::operation)?;
            Ok((entry, journal))
        })?;
        // Opening failures also leave the durable destination available to retry.
        self.open_registered_library(entry.library_id())?;
        self.send_library_list_changed();
        let original = self.state.library_directory.get(&source)?;
        let destination = self.state.library_directory.get(entry.library_id())?;
        let registry = self.state.registry.clone();
        let checkpoint_key = key.clone();
        destination
            .copy_storage_from(&original, journal.progress.clone(), move |progress| {
                journal.progress = progress.clone();
                registry.write_value(&checkpoint_key, &serde_json::to_vec(&journal).map_err(BackendError::operation)?).map_err(BackendError::operation)
            })
            .await?;
        if !keep_journal {
            self.state.registry.remove_value(&key).map_err(BackendError::operation)?;
        }
        self.state.account_wake.send_replace(());
        self.send_library_list_changed();
        Ok(entry)
    }

    pub(super) async fn make_library_local_only(&self, source: LibraryId) -> Result<LibraryEntry, BackendError> {
        let _copy = self.state.storage_copy_lock.lock().await;
        let name = self.library_entry(&source)?.library_name().to_owned();
        let account = self.require_account()?;
        let identity = format!("{}|{}", account.server_url(), account.user_id());
        self.state.library_directory.get(&source)?.dispatch(LibraryCommand::Pending(PendingCommand::RefreshStorageCopy)).await?.take::<()>()?;
        let entry = self.copy_library_storage_inner(source, false, true).await?;
        // Only a complete verified copy permits cloud removal. Re-check the
        // captured account on every attempt, including token-refresh retries.
        self.state
            .account
            .request(|session| {
                let matches = format!("{}|{}", session.server_url(), session.user_id()) == identity;
                Box::pin(async move {
                    if !matches {
                        return Err(account_client::AuthError::SignInRequired("sign in to the account that owns this library"));
                    }
                    match account_client::delete_library(session, &source).await.map(|_| ()) {
                        Err(error) if error.is_not_found() => Ok(()),
                        result => result,
                    }
                })
            })
            .await
            .map_err(super::account::account_error)?;
        let destination_id = *entry.library_id();
        let entry = self.with_storage(move |this| {
            if this.state.registry.entry(&source).map_err(BackendError::operation)?.is_some() {
                this.state.registry.begin_local_removal(&source).map_err(BackendError::operation)?;
                this.state.library_directory.retire(&source)?;
            }
            this.state.registry.complete_server_deletion(&source).map_err(BackendError::operation)?;
            this.state.registry.remove_value(&format!("storage-copy:v1:{source}:0")).map_err(BackendError::operation)?;
            let entry = this.state.registry.attach_remote(&destination_id, &name).map_err(BackendError::operation)?;
            if let Some(handle) = this.state.library_directory.try_get(&destination_id)? {
                let _ = handle.commands.try_send(LibraryActorCommand::Control(LibraryControl::Rename { name, refresh_remote: false }));
            }
            Ok(entry)
        })?;
        if let Err(error) = self.finish_local_purge(source).await {
            log::warn!("cloud library was saved locally; old cache cleanup will retry: {error}");
        }
        self.state.account_wake.send_replace(());
        self.send_library_list_changed();
        Ok(entry)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::app::data::{AppDataLocation, BackendContext};
    use library_backend::LibraryClient;
    use library_backend::{LibraryCommand, PendingCommand};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use sync_common::ROOT_DIR_ID;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const LOCAL_EPUB: &[u8] = include_bytes!("../../../library-backend/tests/fixtures/cloud-cover.epub");
    const REMOTE_EPUB: &[u8] = include_bytes!("../../../library-backend/crates/book-inspection/tests/fixtures/malformed-language.epub");

    #[tokio::test]
    async fn cloud_removal_saves_all_books_first_and_failed_transfers_resume_without_deleting_cloud() {
        let root = tempfile::tempdir().unwrap();
        let copied_root = root.path().join("libraries/Source (local copy)");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_url: account_client::Url = format!("http://{}", listener.local_addr().unwrap()).parse().unwrap();
        let fail_reads = Arc::new(AtomicBool::new(true));
        let fail_deletes = Arc::new(AtomicBool::new(true));
        let deletions = Arc::new(AtomicUsize::new(0));
        let server_fail_reads = fail_reads.clone();
        let server_fail_deletes = fail_deletes.clone();
        let server_deletions = deletions.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if stream.read_exact(&mut byte).await.is_err() {
                        break;
                    }
                    request.extend(byte);
                }
                if !request.ends_with(b"\r\n\r\n") {
                    continue;
                }
                let request = String::from_utf8(request).unwrap();
                let length = request.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length: ").and_then(|value| value.parse::<usize>().ok())).unwrap_or(0);
                let mut request_body = vec![0; length];
                if stream.read_exact(&mut request_body).await.is_err() {
                    continue;
                }
                let mut words = request.split_whitespace();
                let method = words.next().unwrap();
                let path = words.next().unwrap();
                let (status, extra_headers, body) = if method == "DELETE" {
                    assert_eq!(std::fs::read(copied_root.join("Book.epub")).unwrap(), LOCAL_EPUB, "cloud deletion must wait for the downloaded book to be copied");
                    assert_eq!(std::fs::read(copied_root.join("Remote.epub")).unwrap(), REMOTE_EPUB, "cloud deletion must wait for the server-only book to be copied");
                    server_deletions.fetch_add(1, Ordering::SeqCst);
                    let id = path.rsplit('/').next().unwrap().parse().unwrap();
                    if server_fail_deletes.load(Ordering::SeqCst) {
                        ("503 Service Unavailable", String::new(), Vec::new())
                    } else {
                        ("200 OK", format!("Content-Type: {}\r\n", sync_common::transport::MEDIA_TYPE), sync_common::transport::encode(&sync_common::api::libraries::DeleteLibraryResponse { library_id: id }).unwrap())
                    }
                } else if method == "GET" && path == "/api/account/snapshot" {
                    let response = account_contract::AccountSnapshotResponse {
                        libraries: Vec::new(),
                        deleted_library_ids: Vec::new(),
                        storage: account_contract::StorageUsageResponse { used_bytes: 0, reserved_bytes: 0, quota_bytes: u64::MAX },
                        library_storage: Vec::new(),
                    };
                    ("200 OK", format!("Content-Type: {}\r\n", sync_common::transport::MEDIA_TYPE), sync_common::transport::encode(&response).unwrap())
                } else if method == "POST" && path == "/api/libraries" {
                    let request: sync_common::api::libraries::LibraryNameRequest = sync_common::transport::decode(&request_body, 8 * 1024 * 1024).unwrap();
                    (
                        "200 OK",
                        format!("Content-Type: {}\r\n", sync_common::transport::MEDIA_TYPE),
                        sync_common::transport::encode(&sync_common::api::libraries::LibrarySummary { library_id: request.library_id, library_name: request.library_name, storage: request.storage }).unwrap(),
                    )
                } else if method == "POST" && path == "/api/sync/exchange" {
                    let request: sync_common::SyncExchangeRequest = sync_common::transport::decode(&request_body, 8 * 1024 * 1024).unwrap();
                    let response = sync_common::SyncExchangeResponse {
                        push: sync_common::PushMutationsResponse { rejected: Vec::new() },
                        pull: sync_common::PullStateResponse { mutations: Vec::new(), next_cursor: sync_common::SyncCursor { generation: Some(sync_common::SyncGeneration::from_u128(1)), ..request.cursor }, has_more: false },
                    };
                    ("200 OK", format!("Content-Type: {}\r\n", sync_common::transport::MEDIA_TYPE), sync_common::transport::encode(&response).unwrap())
                } else if server_fail_reads.load(Ordering::SeqCst) || method != "GET" || !path.contains("/blobs/") {
                    ("503 Service Unavailable", String::new(), Vec::new())
                } else {
                    assert_eq!(method, "GET");
                    let hash = path.rsplit('/').next().unwrap();
                    let range = request.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("range: bytes=").map(str::to_owned)).unwrap();
                    let (start, end) = range.split_once('-').unwrap();
                    let start = start.parse::<usize>().unwrap();
                    let end = end.parse::<usize>().unwrap().min(REMOTE_EPUB.len() - 1);
                    ("206 Partial Content", format!("ETag: \"{hash}\"\r\nContent-Range: bytes {start}-{end}/{}\r\n", REMOTE_EPUB.len()), REMOTE_EPUB[start..=end].to_vec())
                };
                let headers = format!("HTTP/1.1 {status}\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                if stream.write_all(headers.as_bytes()).await.is_err() {
                    continue;
                }
                let _ = stream.write_all(&body).await;
            }
        });
        let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
        let backend = AppBackend::account_test_backend(context, server_url.clone());
        let session = |user: &str| account_client::Session::try_new(server_url.clone(), user.into(), format!("{user}@example.com"), "a".repeat(43)).unwrap();
        backend.adopt_session(session("owner")).unwrap();
        let source = backend.create_library_with_asset_storage("Source".into(), true).unwrap();
        let id = *source.library_id();
        let library = LibraryClient::new(id, backend.state.library_directory.get(&id).unwrap());
        let downloaded = library.import_downloaded_book(ROOT_DIR_ID, "Book.epub".into(), LOCAL_EPUB.to_vec()).await.unwrap();
        let remote = library.import_downloaded_book(ROOT_DIR_ID, "Remote.epub".into(), REMOTE_EPUB.to_vec()).await.unwrap();
        library.update_reading_position(downloaded, "epubcfi(/6/2)".into(), Some(42.0), None).await.unwrap();
        backend.state.library_directory.retire(&id).unwrap();
        {
            let db = library_database::Database::open(&backend.state.registry.library_database_locator(&id, source.storage_locator())).unwrap();
            db.record_remote_asset(library_model::BlobKind::Book, &remote).unwrap();
            db.evict_local_book(&remote).unwrap();
            std::fs::remove_file(std::path::Path::new(source.storage_locator()).join("Remote.epub")).unwrap();
        }
        backend.open_registered_library(&id).unwrap();
        let error = backend.set_library_asset_storage(id, false).await.unwrap_err();
        assert!(error.to_string().contains("503"), "{error}");
        assert_eq!(deletions.load(Ordering::SeqCst), 0);
        assert!(backend.library_entry(&id).is_ok(), "failed transfers preserve the cloud library");
        let key = format!("storage-copy:v1:{id}:0");
        let pending: CopyJournal = serde_json::from_slice(&backend.state.registry.read_value(&key).unwrap().unwrap()).unwrap();
        fail_reads.store(false, Ordering::SeqCst);
        assert!(backend.delete_library(&id).await.is_err(), "a failed cloud deletion must retain the completed local copy for retry");
        assert_eq!(deletions.load(Ordering::SeqCst), 1);
        assert!(backend.library_entry(&id).is_ok());
        assert!(backend.state.registry.read_value(&key).unwrap().is_some());
        fail_deletes.store(false, Ordering::SeqCst);
        backend.set_library_asset_storage(id, false).await.unwrap();
        assert_eq!(deletions.load(Ordering::SeqCst), 2);
        assert!(backend.state.registry.entry(&id).unwrap().is_none());
        assert!(backend.state.registry.read_value(&key).unwrap().is_none());
        let entries = backend.libraries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(*entries[0].library_id(), pending.destination, "retry must reuse the existing destination");
        assert_eq!(entries[0].library_name(), "Source", "conversion retains the library's name");
        assert_eq!(backend.state.registry.storage(&pending.destination).unwrap(), LibraryStorage::Local);
        assert!(backend.state.registry.device_only(&pending.destination).unwrap());
        let local = LibraryClient::new(pending.destination, backend.state.library_directory.get(&pending.destination).unwrap());
        assert_eq!(local.book_count().await.unwrap(), 2);
        assert!(local.resolve_book(downloaded).await.is_ok());
        assert!(local.resolve_book(remote).await.is_ok());
        assert_eq!(local.reading_position(downloaded).await.unwrap().as_deref(), Some("epubcfi(/6/2)"));
        backend.logout_with_notification(|_| async {}).unwrap();
        assert_eq!(backend.libraries().unwrap().len(), 1);
        backend.adopt_session(session("other")).unwrap();
        assert_eq!(backend.libraries().unwrap().len(), 1);
        assert!(backend.library_account(pending.destination).current().unwrap().is_none(), "local-only libraries must not adopt the new account");
        assert!(local.resolve_book(remote).await.is_ok());
        server.abort();
    }

    #[tokio::test]
    async fn bokheim_copy_requires_sign_in_before_creating_a_destination() {
        let root = tempfile::tempdir().unwrap();
        let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).unwrap();
        let source = backend.create_library_with_asset_storage("Source".into(), false).unwrap();
        let error = backend.copy_library_storage(*source.library_id(), true).await.unwrap_err();
        assert_eq!(error.failure(), Some(Failure::SignInRequired));
        assert_eq!(backend.libraries().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn local_copy_resumes_a_durable_destination_without_retiring_the_source() {
        let root = tempfile::tempdir().unwrap();
        let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
        let backend = AppBackend::new(context).unwrap();
        let source = backend.create_library_with_asset_storage("Source".into(), false).unwrap();
        let source_id = *source.library_id();
        let handle = backend.state.library_directory.get(&source_id).unwrap();
        let bytes = include_bytes!("../../../library-backend/tests/fixtures/cloud-cover.epub");
        handle.dispatch(LibraryCommand::Pending(PendingCommand::ImportDownloadedBook { parent_id: ROOT_DIR_ID, file_name: "Book.epub".into(), bytes: bytes.to_vec().into() })).await.unwrap();
        let destination = backend.create_library_with_asset_storage("Pending copy".into(), false).unwrap();
        let key = format!("storage-copy:v1:{source_id}:0");
        backend.state.registry.write_value(&key, &serde_json::to_vec(&CopyJournal { destination: *destination.library_id(), progress: Default::default() }).unwrap()).unwrap();
        let copied = backend.copy_library_storage(source_id, false).await.unwrap();
        assert_eq!(copied.library_id(), destination.library_id());
        assert_eq!(backend.libraries().unwrap().len(), 2);
        assert!(backend.state.registry.read_value(&key).unwrap().is_none());
        assert!(backend.state.library_directory.get(&source_id).is_ok());
        let copied_bytes = std::fs::read(std::path::Path::new(copied.storage_locator()).join("Book.epub")).unwrap();
        assert_eq!(copied_bytes, bytes);
        assert!(std::path::Path::new(source.storage_locator()).join("Book.epub").exists());
    }
}
