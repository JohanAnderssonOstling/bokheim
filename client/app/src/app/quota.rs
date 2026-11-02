//! Serialize quota admission with local commits across every open library.
use std::collections::{HashMap, HashSet};

use super::core::AppBackend;
use crate::{BackendError, Failure};
use content_address::ContentHash;
use library_backend::{LibraryCommand, ReadyCommand};
use library_model::CloudQuotaBook;
use library_runtime::quota::CloudQuotaPermit;
use sync_common::BookId;
use sync_common::api::assets::BlobManifestEntry;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ContentBudgetKey {
    Bytes(ContentHash),
    Book(BookId),
}

impl ContentBudgetKey {
    fn new(book: BookId, checksum: Option<ContentHash>) -> Self {
        checksum.or_else(|| book.content_hash()).map(Self::Bytes).unwrap_or(Self::Book(book))
    }
}

impl AppBackend {
    pub(super) async fn admit_cloud_books(&self, books: Vec<BlobManifestEntry>) -> Result<CloudQuotaPermit, BackendError> {
        self.admit_cloud_capacity(books, 0).await
    }

    pub(super) async fn admit_cloud_capacity(&self, books: Vec<BlobManifestEntry>, additional_bytes: u64) -> Result<CloudQuotaPermit, BackendError> {
        let permit = CloudQuotaPermit::new(self.state.cloud_quota_lock.clone().lock_owned().await);
        let session = self.require_account()?;
        let identity = (session.server_url().clone(), session.user_id().to_owned());
        let (mut pending, owned) = self.cloud_book_budget().await?;
        for book in books {
            pending.entry(ContentBudgetKey::new(book.content_hash, book.byte_checksum)).and_modify(|(_, size)| *size = (*size).max(book.size_bytes)).or_insert((book.content_hash, book.size_bytes));
        }
        // Known server books cost no further capacity, including another
        // placement of the same book in a different library.
        let required = pending
            .into_iter()
            .filter(|(hash, _)| !owned.contains(hash))
            .try_fold(additional_bytes, |total, (_, (_, bytes))| total.checked_add(bytes))
            .ok_or_else(|| BackendError::classified(Failure::CloudStorageFull, "the books exceed the cloud storage quota"))?;
        let snapshot = self
            .state
            .account
            .request(|session| {
                let matches = session.server_url() == &identity.0 && session.user_id() == identity.1;
                Box::pin(async move {
                    if !matches {
                        return Err(account_client::AuthError::SignInRequired("the account changed while checking cloud storage"));
                    }
                    account_client::account_snapshot(session).await
                })
            })
            .await
            .map_err(super::account::account_error)?;
        if required > snapshot.storage.available_bytes() {
            return Err(BackendError::classified(Failure::CloudStorageFull, "not enough cloud storage for these books; free space or use a local-only library"));
        }
        Ok(permit)
    }

    async fn cloud_book_inventory(&self) -> Result<Vec<CloudQuotaBook>, BackendError> {
        let mut books = Vec::new();
        for entry in self.libraries()? {
            if self.library_asset_storage_enabled(entry.library_id())? {
                let library = self.state.library_directory.get(entry.library_id())?;
                books.extend(library.dispatch(LibraryCommand::Ready(ReadyCommand::CloudQuotaBooks)).await?.take::<Vec<CloudQuotaBook>>()?);
            }
        }
        Ok(books)
    }

    async fn cloud_book_budget(&self) -> Result<(HashMap<ContentBudgetKey, (BookId, u64)>, HashSet<ContentBudgetKey>), BackendError> {
        let mut pending = HashMap::new();
        let mut owned = HashSet::new();
        for book in self.cloud_book_inventory().await? {
            let key = ContentBudgetKey::new(book.book_id, book.byte_checksum);
            if book.server_has_book {
                owned.insert(key);
            }
            pending.entry(key).and_modify(|(_, size): &mut (BookId, u64)| *size = (*size).max(book.size_bytes)).or_insert((book.book_id, book.size_bytes));
        }
        Ok((pending, owned))
    }

    pub(super) async fn pending_cloud_books(&self) -> Result<HashMap<BookId, u64>, BackendError> {
        let (mut pending, owned) = self.cloud_book_budget().await?;
        pending.retain(|hash, _| !owned.contains(hash));
        Ok(pending.into_values().collect())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) mod tests {
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
    async fn quota_admission_counts_pending_books_across_libraries_and_serializes_concurrent_imports() {
        let root = tempfile::tempdir().unwrap();
        let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
        let (backend, server) = quota_backend(context).await;
        backend.adopt_session(account_client::Session::try_new(server.url.clone(), "owner".into(), "owner@example.com".into(), "a".repeat(43)).unwrap()).unwrap();
        let first = backend.create_library_with_asset_storage("First".into(), true).unwrap();
        let second = backend.create_library_with_asset_storage("Second".into(), true).unwrap();
        let first = LibraryClient::new(*first.library_id(), backend.state.library_directory.get(first.library_id()).unwrap());
        let second = LibraryClient::new(*second.library_id(), backend.state.library_directory.get(second.library_id()).unwrap());
        let a = include_bytes!("../../../library-backend/tests/fixtures/cloud-cover.epub");
        let b = include_bytes!("../../../library-backend/crates/book-inspection/tests/fixtures/malformed-language.epub");
        server.usage.lock().unwrap().quota_bytes = a.len().max(b.len()) as u64;
        let (a_result, b_result) = tokio::join!(first.import_downloaded_book(ROOT_DIR_ID, "First.epub".into(), a.to_vec()), second.import_downloaded_book(ROOT_DIR_ID, "Second.epub".into(), b.to_vec()));
        assert_ne!(a_result.is_ok(), b_result.is_ok(), "only one book fits; concurrent libraries must share the quota");
        let rejected = a_result.as_ref().err().or(b_result.as_ref().err()).unwrap();
        assert_eq!(rejected.failure(), Some(Failure::CloudStorageFull));
        assert_eq!(first.book_count().await.unwrap() + second.book_count().await.unwrap(), 1);

        // A second placement of identical bytes consumes no additional quota.
        let (destination, bytes) = if a_result.is_ok() { (&second, a.as_slice()) } else { (&first, b.as_slice()) };
        destination.import_downloaded_book(ROOT_DIR_ID, "Same book.epub".into(), bytes.to_vec()).await.unwrap();
        assert_eq!(first.book_count().await.unwrap() + second.book_count().await.unwrap(), 2);
        assert_eq!(backend.pending_cloud_books().await.unwrap().len(), 1);
        // Admission is reconstructed from durable book rows after sessions close.
        for entry in backend.libraries().unwrap() {
            backend.state.library_directory.retire(entry.library_id()).unwrap();
        }
        backend.open_registered_libraries().unwrap();
        assert_eq!(backend.pending_cloud_books().await.unwrap().len(), 1);
        let quota_bytes = server.usage.lock().unwrap().quota_bytes;
        let error = backend.admit_cloud_capacity(Vec::new(), quota_bytes).await.err().unwrap();
        assert_eq!(error.failure(), Some(Failure::CloudStorageFull));
    }

    #[tokio::test]
    async fn enabling_cloud_checks_used_and_reserved_quota_without_changing_a_rejected_local_library() {
        let root = tempfile::tempdir().unwrap();
        let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
        let (backend, server) = quota_backend(context).await;
        backend.adopt_session(account_client::Session::try_new(server.url.clone(), "owner".into(), "owner@example.com".into(), "a".repeat(43)).unwrap()).unwrap();
        let entry = backend.create_library_with_asset_storage("Local".into(), false).unwrap();
        let id = *entry.library_id();
        let library = LibraryClient::new(id, backend.state.library_directory.get(&id).unwrap());
        let bytes = include_bytes!("../../../library-backend/tests/fixtures/cloud-cover.epub");
        library.import_downloaded_book(ROOT_DIR_ID, "Book.epub".into(), bytes.to_vec()).await.unwrap();
        for (used_bytes, reserved_bytes) in [(1, 0), (0, 1)] {
            *server.usage.lock().unwrap() = StorageUsageResponse { used_bytes, reserved_bytes, quota_bytes: bytes.len() as u64 };
            let error = backend.set_library_asset_storage(id, true).await.unwrap_err();
            assert_eq!(error.failure(), Some(Failure::CloudStorageFull));
            assert!(backend.state.registry.device_only(&id).unwrap());
            assert!(!backend.library_asset_storage_enabled(&id).unwrap());
            assert_eq!(library.book_count().await.unwrap(), 1);
            let error = backend.copy_library_storage(id, true).await.unwrap_err();
            assert_eq!(error.failure(), Some(Failure::CloudStorageFull));
            assert_eq!(backend.libraries().unwrap().len(), 1, "quota rejection must not create a copy");
        }
        *server.usage.lock().unwrap() = StorageUsageResponse { used_bytes: 0, reserved_bytes: 0, quota_bytes: bytes.len() as u64 };
        backend.set_library_asset_storage(id, true).await.unwrap();
        assert!(backend.library_asset_storage_enabled(&id).unwrap());
    }
}
