use super::*;
use crate::app::data::{AppDataLocation, BackendContext};
use account_contract::{AccountSnapshotResponse, StorageUsageResponse};
use library_backend::LibraryClient;
use std::sync::{Arc, Mutex};
use sync_common::ROOT_DIR_ID;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(in crate::app) struct QuotaServer {
    pub(in crate::app) url: account_client::Url,
    usage: Arc<Mutex<StorageUsageResponse>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for QuotaServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(in crate::app) async fn quota_backend(context: BackendContext) -> (AppBackend, QuotaServer) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url: account_client::Url = format!("http://{}/", listener.local_addr().unwrap()).parse().unwrap();
    let usage = Arc::new(Mutex::new(StorageUsageResponse { used_bytes: 0, reserved_bytes: 0, quota_bytes: u64::MAX }));
    let remote = usage.clone();
    let task = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let usage = remote.clone();
            tokio::spawn(async move {
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    if stream.read_exact(&mut byte).await.is_err() {
                        return;
                    }
                    header.push(byte[0]);
                    if header.len() > 65536 {
                        return;
                    }
                }
                let request = String::from_utf8(header).unwrap();
                let (status, body) = if request.starts_with("GET /api/account/snapshot ") {
                    let storage = *usage.lock().unwrap();
                    ("200 OK", sync_common::transport::encode(&AccountSnapshotResponse { libraries: Vec::new(), deleted_library_ids: Vec::new(), storage, library_storage: Vec::new() }).unwrap())
                } else {
                    ("503 Service Unavailable", Vec::new())
                };
                let header = format!("HTTP/1.1 {status}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", sync_common::transport::MEDIA_TYPE, body.len());
                if stream.write_all(header.as_bytes()).await.is_ok() {
                    let _ = stream.write_all(&body).await;
                }
            });
        }
    });
    let backend = AppBackend::account_test_backend(context, url.clone());
    (backend, QuotaServer { url, usage, task })
}

#[tokio::test]
async fn quota_admission_counts_pending_books_across_libraries_and_serializes_enabling_cloud() {
    let root = tempfile::tempdir().unwrap();
    let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
    let (backend, server) = quota_backend(context).await;
    backend.state.account_service.adopt_session(account_client::Session::try_new(server.url.clone(), "owner".into(), "owner@example.com".into(), "a".repeat(43)).unwrap()).unwrap();
    let first = backend.state.library_service.create_with_asset_storage("First".into(), false).unwrap();
    let second = backend.state.library_service.create_with_asset_storage("Second".into(), false).unwrap();
    let first = LibraryClient::new(*first.library_id(), backend.state.library_directory.get(first.library_id()).unwrap());
    let second = LibraryClient::new(*second.library_id(), backend.state.library_directory.get(second.library_id()).unwrap());
    let a = include_bytes!("../../../../library-backend/tests/fixtures/cloud-cover.epub");
    let b = include_bytes!("../../../../library-backend/crates/book-inspection/tests/fixtures/malformed-language.epub");
    server.usage.lock().unwrap().quota_bytes = a.len().max(b.len()) as u64;
    let (a_result, b_result) = tokio::join!(first.import_downloaded_book(ROOT_DIR_ID, "First.epub".into(), a.to_vec()), second.import_downloaded_book(ROOT_DIR_ID, "Second.epub".into(), b.to_vec()));
    // Local imports remain available independently of cloud capacity.
    a_result.unwrap();
    b_result.unwrap();
    let (a_result, b_result) = tokio::join!(backend.set_library_asset_storage(*first.id(), true), backend.set_library_asset_storage(*second.id(), true));
    assert_ne!(a_result.is_ok(), b_result.is_ok(), "only one library fits; concurrent libraries must share the quota");
    let rejected = a_result.as_ref().err().or(b_result.as_ref().err()).unwrap();
    assert_eq!(rejected.failure(), Some(Failure::CloudStorageFull));
    assert_eq!(first.book_count().await.unwrap() + second.book_count().await.unwrap(), 2);
    assert_eq!(backend.state.library_service.library_asset_storage_enabled(first.id()).unwrap(), a_result.is_ok());
    assert_eq!(backend.state.library_service.library_asset_storage_enabled(second.id()).unwrap(), b_result.is_ok());

    // A second placement of identical bytes consumes no additional quota.
    let destination = backend.state.library_service.create_with_asset_storage("Same book".into(), false).unwrap();
    let destination = LibraryClient::new(*destination.library_id(), backend.state.library_directory.get(destination.library_id()).unwrap());
    let bytes = if a_result.is_ok() { a.as_slice() } else { b.as_slice() };
    destination.import_downloaded_book(ROOT_DIR_ID, "Same book.epub".into(), bytes.to_vec()).await.unwrap();
    backend.set_library_asset_storage(*destination.id(), true).await.unwrap();
    assert_eq!(destination.book_count().await.unwrap(), 1);
    assert_eq!(backend.state.library_service.pending_cloud_books().await.unwrap().len(), 1);
    // Admission is reconstructed from durable book rows after sessions close.
    for entry in backend.libraries().unwrap() {
        backend.state.library_directory.retire(entry.library_id()).unwrap();
    }
    backend.open_registered_libraries().unwrap();
    assert_eq!(backend.state.library_service.pending_cloud_books().await.unwrap().len(), 1);
    let quota_bytes = server.usage.lock().unwrap().quota_bytes;
    let error = backend.state.library_service.admit_cloud_capacity(Vec::new(), quota_bytes).await.err().unwrap();
    assert_eq!(error.failure(), Some(Failure::CloudStorageFull));
}

#[tokio::test]
async fn enabling_cloud_checks_used_and_reserved_quota_without_changing_a_rejected_local_library() {
    let root = tempfile::tempdir().unwrap();
    let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
    let (backend, server) = quota_backend(context).await;
    backend.state.account_service.adopt_session(account_client::Session::try_new(server.url.clone(), "owner".into(), "owner@example.com".into(), "a".repeat(43)).unwrap()).unwrap();
    let entry = backend.state.library_service.create_with_asset_storage("Local".into(), false).unwrap();
    let id = *entry.library_id();
    let library = LibraryClient::new(id, backend.state.library_directory.get(&id).unwrap());
    let bytes = include_bytes!("../../../../library-backend/tests/fixtures/cloud-cover.epub");
    library.import_downloaded_book(ROOT_DIR_ID, "Book.epub".into(), bytes.to_vec()).await.unwrap();
    for (used_bytes, reserved_bytes) in [(1, 0), (0, 1)] {
        *server.usage.lock().unwrap() = StorageUsageResponse { used_bytes, reserved_bytes, quota_bytes: bytes.len() as u64 };
        let error = backend.set_library_asset_storage(id, true).await.unwrap_err();
        assert_eq!(error.failure(), Some(Failure::CloudStorageFull));
        assert!(backend.state.registry.device_only(&id).unwrap());
        assert!(!backend.state.library_service.library_asset_storage_enabled(&id).unwrap());
        assert_eq!(library.book_count().await.unwrap(), 1);
        let error = backend.copy_library_storage(id, true).await.unwrap_err();
        assert_eq!(error.failure(), Some(Failure::CloudStorageFull));
        assert_eq!(backend.libraries().unwrap().len(), 1, "quota rejection must not create a copy");
    }
    *server.usage.lock().unwrap() = StorageUsageResponse { used_bytes: 0, reserved_bytes: 0, quota_bytes: bytes.len() as u64 };
    backend.set_library_asset_storage(id, true).await.unwrap();
    assert!(backend.state.library_service.library_asset_storage_enabled(&id).unwrap());
}
