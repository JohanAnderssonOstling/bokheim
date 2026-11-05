use super::*;
use crate::app::data::{AppDataLocation, BackendContext};
use crate::{BackendError, Failure};
use library_backend::LibraryClient;
use library_management::account::account_error;
use library_management::management::StorageCopyJournal;
use library_registry::LibraryEntry;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use sync_common::ROOT_DIR_ID;
use sync_common::{storage::LibraryStorage, LibraryId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const LOCAL_EPUB: &[u8] = include_bytes!("../../../../library-backend/tests/fixtures/cloud-cover.epub");
const REMOTE_EPUB: &[u8] = include_bytes!("../../../../library-backend/crates/book-inspection/tests/fixtures/malformed-language.epub");

#[tokio::test]
async fn cloud_storage_changes_preserve_identity_and_retry_failed_downloads() {
    let root = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_url: account_client::Url = format!("http://{}", listener.local_addr().unwrap()).parse().unwrap();
    let fail_reads = Arc::new(AtomicBool::new(true));
    let deletions = Arc::new(AtomicUsize::new(0));
    let server_fail_reads = fail_reads.clone();
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
                server_deletions.fetch_add(1, Ordering::SeqCst);
                let id = path.rsplit('/').next().unwrap().parse().unwrap();
                ("200 OK", format!("Content-Type: {}\r\n", sync_common::transport::MEDIA_TYPE), sync_common::transport::encode(&sync_common::api::libraries::DeleteLibraryResponse { library_id: id }).unwrap())
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
                let checksum = content_address::hash_reader(&mut std::io::Cursor::new(REMOTE_EPUB)).unwrap();
                let headers = format!("ETag: \"{hash}\"\r\nx-bokheim-content-hash: {checksum}\r\n");
                if let Some(range) = request.lines().find_map(|line| line.to_ascii_lowercase().strip_prefix("range: bytes=").map(str::to_owned)) {
                    let (start, end) = range.split_once('-').unwrap();
                    let start = start.parse::<usize>().unwrap();
                    let end = end.parse::<usize>().unwrap().min(REMOTE_EPUB.len() - 1);
                    ("206 Partial Content", format!("{headers}Content-Range: bytes {start}-{end}/{}\r\n", REMOTE_EPUB.len()), REMOTE_EPUB[start..=end].to_vec())
                } else {
                    ("200 OK", headers, REMOTE_EPUB.to_vec())
                }
            };
            let headers = format!("HTTP/1.1 {status}\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            if stream.write_all(headers.as_bytes()).await.is_err() {
                continue;
            }
            let _ = stream.write_all(&body).await;
        }
    });
    let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
    let backend = AppBackend::account_test_backend(context, server_url.clone()).await;
    let session = |user: &str| account_client::Session::try_new(server_url.clone(), user.into(), format!("{user}@example.com"), "a".repeat(43), i64::MAX - 1, "r".repeat(43), i64::MAX).unwrap();
    backend.account_service.adopt_session(session("owner")).await.unwrap();
    let source = backend.library_service.create_with_asset_storage("Source".into(), true).await.unwrap();
    let id = *source.library_id();
    let library = LibraryClient::new(backend.library_service.handle(&id).unwrap(), Arc::new(()));
    let downloaded = library.import_downloaded_book(ROOT_DIR_ID, "Book.epub".into(), LOCAL_EPUB.to_vec()).await.unwrap();
    let remote = library.import_downloaded_book(ROOT_DIR_ID, "Remote.epub".into(), REMOTE_EPUB.to_vec()).await.unwrap();
    library.update_reading_position(downloaded, "epubcfi(/6/2)".into(), Some(42.0), None).await.unwrap();
    backend.library_service.retire(&id);
    {
        let db = library_database::Database::open(&backend.registry.library_database_locator(&id, source.storage_locator())).unwrap();
        db.record_remote_asset(library_model::BlobKind::Book, &remote).unwrap();
        db.evict_local_book(&remote).unwrap();
        std::fs::remove_file(std::path::Path::new(source.storage_locator()).join("Remote.epub")).unwrap();
    }
    backend.library_service.open(&id).await.unwrap();
    let error = backend.library_service.set_library_asset_storage(id, false).await.unwrap_err();
    assert!(error.to_string().contains("503"), "{error}");
    assert_eq!(deletions.load(Ordering::SeqCst), 0);
    assert!(backend.library_service.entry(&id).await.is_ok(), "failed transfers preserve the cloud library");
    assert!(backend.library_service.library_asset_storage_enabled(&id).await.unwrap(), "failed downloads must preserve cloud storage");
    assert_eq!(backend.library_service.list().await.unwrap().len(), 1, "the toggle must never create a copy");
    fail_reads.store(false, Ordering::SeqCst);
    backend.library_service.set_library_asset_storage(id, false).await.unwrap();
    assert_eq!(deletions.load(Ordering::SeqCst), 0, "turning off book storage must preserve the cloud library and metadata");
    let entries = backend.library_service.list().await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(*entries[0].library_id(), id);
    assert_eq!(entries[0].library_name(), "Source");
    assert_eq!(entries[0].storage_locator(), source.storage_locator());
    assert_eq!(backend.registry.storage(&id).unwrap(), LibraryStorage::Local);
    assert!(!backend.registry.device_only(&id).unwrap());
    assert!(backend.registry.has_account(&id).unwrap());
    let account = backend.library_service.open_options(id).await.unwrap().0.account;
    assert!(account.current().unwrap().is_some(), "metadata synchronization must keep its account");
    assert!(backend.registry.read_value(&format!("storage-copy:v1:{id}:0")).unwrap().is_none());
    let pending = backend.registry.read_value(&format!("pending-cloud-storage:{id}")).unwrap().unwrap();
    assert!(std::str::from_utf8(&pending).unwrap().ends_with(":0"));
    let local = LibraryClient::new(backend.library_service.handle(&id).unwrap(), Arc::new(()));
    assert_eq!(local.book_count().await.unwrap(), 2);
    assert!(local.resolve_book(downloaded).await.is_ok());
    assert!(local.resolve_book(remote).await.is_ok());
    assert_eq!(local.reading_position(downloaded).await.unwrap().as_deref(), Some("epubcfi(/6/2)"));
    assert_eq!(std::fs::read(std::path::Path::new(source.storage_locator()).join("Remote.epub")).unwrap(), REMOTE_EPUB);
    backend.library_service.set_library_asset_storage(id, true).await.unwrap();
    assert!(backend.library_service.library_asset_storage_enabled(&id).await.unwrap());
    assert_eq!(*backend.library_service.list().await.unwrap()[0].library_id(), id);
    backend.account_service.logout_with_notification(|_| async {}).await.unwrap();
    assert_eq!(backend.library_service.list().await.unwrap().len(), 1);
    backend.account_service.adopt_session(session("other")).await.unwrap();
    assert!(backend.library_service.list().await.unwrap().is_empty(), "changing book storage must preserve the original account owner");
    assert!(account.current().unwrap().is_none());
    server.abort();
}

#[tokio::test]
async fn bokheim_copy_requires_sign_in_before_creating_a_destination() {
    let root = tempfile::tempdir().unwrap();
    let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).await.unwrap();
    let source = backend.library_service.create_with_asset_storage("Source".into(), false).await.unwrap();
    let error = backend.library_service.copy_library_storage(*source.library_id(), true).await.unwrap_err();
    assert_eq!(error.failure(), Some(Failure::SignInRequired));
    assert_eq!(backend.library_service.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn local_copy_resumes_a_durable_destination_without_retiring_the_source() {
    let root = tempfile::tempdir().unwrap();
    let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
    let backend = AppBackend::new(context).await.unwrap();
    let source = backend.library_service.create_with_asset_storage("Source".into(), false).await.unwrap();
    let source_id = *source.library_id();
    let handle = backend.library_service.handle(&source_id).unwrap();
    let bytes = include_bytes!("../../../../library-backend/tests/fixtures/cloud-cover.epub");
    handle.import_downloaded_book(ROOT_DIR_ID, "Book.epub".into(), bytes.to_vec().into()).await.unwrap();
    let destination = backend.library_service.create_with_asset_storage("Pending copy".into(), false).await.unwrap();
    let key = format!("storage-copy:v1:{source_id}:0");
    backend.registry.write_value(&key, &serde_json::to_vec(&StorageCopyJournal { destination: *destination.library_id(), progress: Default::default() }).unwrap()).unwrap();
    let copied = backend.library_service.copy_library_storage(source_id, false).await.unwrap();
    assert_eq!(copied.library_id(), destination.library_id());
    assert_eq!(backend.library_service.list().await.unwrap().len(), 2);
    assert!(backend.registry.read_value(&key).unwrap().is_none());
    assert!(backend.library_service.handle(&source_id).is_ok());
    let copied_bytes = std::fs::read(std::path::Path::new(copied.storage_locator()).join("Book.epub")).unwrap();
    assert_eq!(copied_bytes, bytes);
    assert!(std::path::Path::new(source.storage_locator()).join("Book.epub").exists());
}
