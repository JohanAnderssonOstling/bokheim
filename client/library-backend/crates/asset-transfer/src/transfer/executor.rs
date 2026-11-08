//! Book and cover transfers with independent metadata commands.
#[cfg(not(target_arch = "wasm32"))]
use crate::deadline::download_deadline;
use crate::TransferError;
use crate::{deadline::transfer_deadline, policy::SMALL_REQUEST_TIMEOUT};
use content_address::ContentHash;
use futures_util::{stream, StreamExt};
use library_database::DatabaseClient;
use library_model::BookUploadIntent;
#[cfg(not(target_arch = "wasm32"))]
use library_model::{DownloadState, LibraryUpdate, TransferOrigin};
use library_placement::LibraryState;
use library_services::{
    account::LibraryAuthentication,
    events::{LibraryEvent, LibraryEventRouter},
    interest::NotificationInterest,
};
use std::{
    collections::HashSet,
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use sync_common::{BookId, LibraryId};
use sync_transport::SyncCredentials;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AssetExecutor {
    pub(super) asset_storage_enabled: Arc<AtomicBool>,
    pub(crate) http_client: reqwest::Client,
    pub(crate) library_id: LibraryId,
    pub(crate) events: LibraryEventRouter,
    pub(super) authentication: LibraryAuthentication,
    pub(super) notification_interest: NotificationInterest,
    pub(super) availability_changed: Arc<AtomicBool>,
    pub(crate) state: Arc<RwLock<LibraryState>>,
    pub(crate) database: DatabaseClient,
    pub(crate) cpu: Arc<dyn cpu_host::CpuHost>,
}

/// Poll uploads on the library thread. Ordinary failures leave later work
/// running; lost authentication stops the batch and drops in-flight requests.
async fn collect_uploads<F>(uploads: impl IntoIterator<Item = F>, concurrency: usize) -> Vec<Result<(), TransferError>>
where
    F: Future<Output = Result<(), TransferError>>,
{
    let uploads = stream::iter(uploads).buffer_unordered(concurrency);
    tokio::pin!(uploads);
    let mut results = Vec::new();
    while let Some(result) = uploads.next().await {
        let authentication = matches!(result, Err(TransferError::AuthenticationRequired));
        results.push(result);
        if authentication {
            break;
        }
    }
    results
}

impl AssetExecutor {
    /// Keep metadata commands independent of file admission and pair renewal
    /// with its credentials. State supplies the current filesystem handles.
    pub fn new(
        state: Arc<RwLock<LibraryState>>, database: DatabaseClient, events: LibraryEventRouter, library_id: LibraryId, authentication: LibraryAuthentication, notification_interest: NotificationInterest, cpu: Arc<dyn cpu_host::CpuHost>,
    ) -> Self {
        Self {
            asset_storage_enabled: Arc::new(AtomicBool::new(true)),
            http_client: http_response::client(),
            library_id,
            events,
            authentication,
            notification_interest,
            availability_changed: Arc::new(AtomicBool::new(false)),
            state,
            database,
            cpu,
        }
    }

    pub fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }

    pub fn set_asset_storage_enabled(&self, enabled: bool) {
        self.asset_storage_enabled.store(enabled, Ordering::Release);
    }

    pub fn asset_storage_enabled(&self) -> bool {
        self.asset_storage_enabled.load(Ordering::Acquire)
    }

    async fn negotiate_uploads(&self, intents: &[BookUploadIntent]) -> Result<sync_common::api::assets::BlobManifestResponse, TransferError> {
        let credentials = self.credentials().await?;
        let manifest = intents.iter().map(|intent| sync_common::api::assets::BlobManifestEntry { byte_checksum: intent.byte_checksum, book_id: intent.book_id, size_bytes: intent.size_bytes }).collect();
        let response = transfer_deadline(SMALL_REQUEST_TIMEOUT, crate::negotiate_blobs(&self.http_client, &credentials, &self.library_id, manifest)).await?.map_err(|error| match error {
            sync_transport::SyncRequestError::AuthenticationRequired => TransferError::AuthenticationRequired,
            error => TransferError::retryable(error),
        })?;
        self.check_transfer_account(&credentials).await?;
        for intent in intents.iter().filter(|intent| response.owned.contains(&intent.book_id)) {
            let endpoint = sync_transport::blob_endpoint(credentials.server_url(), &self.library_id, &intent.book_id).map_err(crate::AssetResponseError::Url)?;
            let head = transfer_deadline(SMALL_REQUEST_TIMEOUT, self.http_client.head(endpoint).bearer_auth(credentials.access_token()).send()).await?.map_err(TransferError::retryable)?;
            let head = crate::validate_streaming_asset_response(head)?;
            let checksum = head
                .headers()
                .get("x-bokheim-content-hash")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<ContentHash>().ok())
                .or_else(|| intent.book_id.content_hash())
                .ok_or_else(|| TransferError::rejected("the server did not provide the canonical book checksum"))?;
            self.check_transfer_account(&credentials).await?;
            let size = head.headers().get(reqwest::header::CONTENT_LENGTH).and_then(|value| value.to_str().ok()).and_then(|value| value.parse::<u64>().ok());
            self.database.confirm_uploaded_content_with_pdf(intent.book_id, checksum, size, None).await?;
        }
        Ok(response)
    }

    /// Negotiate once per page; native uploads run four at a time, browser
    /// uploads sequentially. Every book writes as soon as its request finishes.
    pub async fn upload_books(&self, intents: &[BookUploadIntent]) -> Result<Vec<Result<(), TransferError>>, TransferError> {
        if !self.asset_storage_enabled() || intents.is_empty() {
            return Ok(Vec::new());
        }
        let _interest = self.notification_interest.work();
        let response = self.refreshed(|| self.negotiate_uploads(intents)).await?;
        let owned = intents.iter().filter(|intent| response.owned.contains(&intent.book_id)).cloned().collect();
        self.database.settle_book_uploads(owned).await?;
        let uploads: Vec<_> = intents
            .iter()
            .map(|intent: &BookUploadIntent| {
                let present = response.owned.contains(&intent.book_id);
                let rejected = response.rejected.iter().find(|item| item.book_id == intent.book_id).map(|item| TransferError::from_upload_admission(item.reason));
                async move {
                    if let Some(error) = rejected {
                        Err(error)
                    } else if present {
                        Ok(())
                    } else {
                        self.refreshed(|| self.upload_book(intent)).await
                    }
                }
            })
            .collect();
        let concurrency = if cfg!(target_arch = "wasm32") { 1 } else { 4 };
        Ok(collect_uploads(uploads, concurrency).await)
    }

    async fn upload_book(&self, intent: &BookUploadIntent) -> Result<(), TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(());
        }
        let snapshot = self.database.transfer_snapshot(&[intent.book_id]).await?;
        self.upload_blob_version(&snapshot, intent).await
    }

    /// Presence discovery also serves local extraction, which skips books with
    /// server covers. Validate the response before changing any presence facts.
    pub async fn cover_presence(&self, hashes: &[BookId]) -> Result<Vec<BookId>, TransferError> {
        self.refreshed(|| async {
            let credentials = self.credentials().await?;
            let response = transfer_deadline(SMALL_REQUEST_TIMEOUT, sync_transport::thumbnail_presence(&self.http_client, &credentials, &self.library_id, hashes.to_vec())).await?.map_err(|error| match error {
                sync_transport::SyncRequestError::AuthenticationRequired => TransferError::AuthenticationRequired,
                error => TransferError::retryable(error),
            })?;
            let mut missing: HashSet<_> = hashes.iter().copied().collect();
            if response.present.iter().any(|hash| !missing.remove(hash)) {
                return Err(TransferError::retryable("invalid thumbnail presence response"));
            }
            let missing = hashes.iter().filter(|hash| missing.contains(hash)).copied().collect::<Vec<_>>();
            self.check_transfer_account(&credentials).await?;
            self.database.record_cover_presence(&response.present, &missing).await?;
            Ok(missing)
        })
        .await
    }

    pub async fn upload_covers(&self, hashes: &[BookId]) -> Result<Vec<Result<(), TransferError>>, TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(Vec::new());
        }
        let _interest = self.notification_interest.work();
        let missing = self.cover_presence(hashes).await?;
        if missing.is_empty() {
            return Ok(Vec::new());
        }
        match self.refreshed(|| self.upload_thumbnail_batch(&missing)).await {
            Err(TransferError::Rejected(_)) if missing.len() > 1 => {
                let uploads: Vec<_> = missing.iter().map(|hash| self.refreshed(move || self.upload_thumbnail_batch(std::slice::from_ref(hash)))).collect();
                Ok(collect_uploads(uploads, 1).await)
            }
            result => Ok(vec![result]),
        }
    }

    pub async fn download_covers(&self, hashes: &[BookId]) -> Result<(), TransferError> {
        let _interest = self.notification_interest.work();
        self.refreshed(|| self.download_thumbnail_batch(hashes)).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn download_book(&self, hash: BookId) -> Result<(), TransferError> {
        self.download_book_mode(hash, false).await
    }

    /// Fetch verified cloud bytes even when an invalid local file exists.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn redownload_book(&self, hash: BookId) -> Result<(), TransferError> {
        self.download_book_mode(hash, true).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn download_book_mode(&self, hash: BookId, force: bool) -> Result<(), TransferError> {
        let origin = self.database.book_download_origin(&hash).await?.unwrap_or(TransferOrigin::Background);
        let result = self.download_book_with_origin(hash, origin, force).await;
        if let Err(error) = &result {
            let storage = self.state.read().await.clone();
            self.report_download_failure(&storage, hash, origin, error).await;
        }
        result
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn report_download_failure(&self, storage: &LibraryState, hash: BookId, origin: TransferOrigin, error: &TransferError) {
        if origin == TransferOrigin::UserInitiated {
            // Alert even if a full disk also prevents clearing the request.
            // Local failures must not forget that the server holds this book.
            log::warn!("user download of {hash} failed: {error}");
            self.events.updates.publish(LibraryUpdate::DownloadFailed { book_id: hash, failure: error.failure() });
            if let Err(error) = self.database.complete_download(&hash).await {
                log::warn!("could not clear failed user download {hash}: {error}");
            }
        }
        if let Err(error) = self.publish_download_status(storage, hash).await {
            log::warn!("could not publish failed download status for {hash}: {error}");
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn download_book_with_origin(&self, hash: BookId, origin: TransferOrigin, force: bool) -> Result<(), TransferError> {
        let _interest = self.notification_interest.work();
        let snapshot = {
            let storage = self.state.read().await.clone();
            let paths = storage.database().book_paths(&hash).await?;
            if !force && storage.assets().book_exists(&hash, &paths).await? {
                storage.database().complete_local_book_request(&hash).await?;
                self.publish_download_status(&storage, hash).await?;
                return Ok(());
            }
            storage.database().transfer_snapshot(&[hash]).await?
        };
        {
            let placement = snapshot.placements.iter().find(|entry| entry.book_id == hash && entry.book.is_some());
            if let Some(entry) = placement {
                let placement = library_model::BookPlacement::new(hash, entry.rel_path.clone());
                let result = self.refreshed(|| async { download_deadline(origin, crate::policy::BOOK_DOWNLOAD_TIMEOUT, Box::pin(self.download_blob(&snapshot, &hash, &placement))).await? }).await;
                if matches!(result, Err(TransferError::Rejected(_))) {
                    self.database.complete_download(&hash).await?;
                }
                if let Err(error) = result {
                    return Err(error);
                }
            } else {
                self.database.complete_download(&hash).await?;
            }
        }
        let storage = self.state.read().await.clone();
        self.publish_download_status(&storage, hash).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn publish_download_status(&self, storage: &LibraryState, hash: BookId) -> Result<(), TransferError> {
        let present = storage.assets().book_exists(&hash, &storage.database().book_paths(&hash).await?).await?;
        let state = storage.database().transfer_download_state(&hash, present).await?;
        self.send_download_status(hash, state);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn send_download_status(&self, book_id: BookId, state: DownloadState) {
        if matches!(state, DownloadState::Downloaded | DownloadState::NotDownloaded) {
            self.availability_changed.store(true, Ordering::Release);
        }
        self.events.download_status_changed(book_id, state);
    }

    pub fn flush_availability_changes(&self) {
        if self.availability_changed.swap(false, Ordering::AcqRel) {
            self.events.emit(LibraryEvent::ContentsChanged);
        }
    }

    pub(crate) async fn credentials(&self) -> Result<SyncCredentials, TransferError> {
        self.authentication.credentials().await.ok_or(TransferError::AuthenticationRequired)
    }

    pub(crate) async fn check_transfer_account(&self, credentials: &SyncCredentials) -> Result<(), TransferError> {
        if self.credentials().await?.server_url() != credentials.server_url() {
            return Err(TransferError::retryable("credentials changed during asset transfer"));
        }
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "executor/upload_tests.rs"]
mod upload_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod failure_tests {
    use super::*;
    use account_client::{Session, SessionPersistence};
    use book_inspection::InspectedBook;
    use book_model::{BookDetails, BookFormat, BookRecord};
    use client_platform_runtime::executor::BoxedBackendFuture;
    use client_protocol::BackendError;
    use cpu_host::{BookTask, BoxedCpuReader, CpuHost, CpuRequest, CpuResponse, ThumbnailBytes};
    use library_database::{executor::BackendExecutor, DatabaseClient, ImportCommit};
    use library_files::AssetStore;
    use library_model::{BlobKind, DownloadState, PublishedStagedBook};
    use library_services::{account::LibraryAccountSession, events::LibraryEventState};
    use std::{
        cell::Cell,
        io::{self, Error, ErrorKind},
        sync::RwLock,
    };
    use sync_common::{
        api::assets::{ThumbnailBatchDownloadResponse, ThumbnailBatchEntry},
        ROOT_DIR_ID,
    };
    use thumbnail::ThumbnailVersions;

    struct UnusedCpu;
    impl CpuHost for UnusedCpu {
        fn book(&self, _: BookTask, _: BoxedCpuReader) -> BoxedBackendFuture<'_, Result<CpuResponse, BackendError>> {
            Box::pin(async { Err("unexpected CPU work".into()) })
        }
        fn request(&self, _: CpuRequest) -> BoxedBackendFuture<'_, Result<CpuResponse, BackendError>> {
            Box::pin(async { Err("unexpected CPU work".into()) })
        }
        fn generate_thumbnail(&self, _: String, _: BoxedCpuReader) -> BoxedBackendFuture<'_, Result<Option<ThumbnailBytes>, BackendError>> {
            Box::pin(async { Err("unexpected CPU work".into()) })
        }
    }

    async fn fixture(origin: TransferOrigin) -> (tempfile::TempDir, AssetExecutor, LibraryEventState, BookId) {
        let root = tempfile::tempdir().unwrap();
        let database = DatabaseClient::open(":memory:", &BackendExecutor::new().unwrap()).await.unwrap();
        database.initialize_library().await.unwrap();
        let hash = BookId::from_bytes([7; 32]);
        database
            .commit_import(ImportCommit {
                source_identity: None,
                byte_checksum: None,
                audiobook_content_hash: None,
                parent_id: ROOT_DIR_ID,
                file_name: "book.epub".into(),
                format: BookFormat::Epub,
                book_id: hash,
                size_bytes: 4,
                inspection_version: 1,
                inspection: Some(InspectedBook { pdf: None, audiobook: None, metadata: BookRecord { metadata: BookDetails { title: "Remote book".into(), ..Default::default() }, description: String::new() }, toc: vec![] }),
                published: PublishedStagedBook { name: "book.epub".into(), relative_path: "/book.epub".into(), published: false, fingerprint: None },
                request_thumbnail: false,
                restore_paths: vec![],
            })
            .await
            .unwrap();
        database.record_remote_asset(BlobKind::Book, &hash).await.unwrap();
        database.sync_request_book_downloads(&[hash], origin).await.unwrap();
        let (events, state) = LibraryEventRouter::new();
        let placement = Arc::new(tokio::sync::RwLock::new(LibraryState::new(database.clone(), AssetStore::open(root.path().to_str().unwrap()).unwrap(), events)));
        let executor = AssetExecutor::new(placement.clone(), database, placement.read().await.events().clone(), LibraryId::new_v4(), LibraryAuthentication::Credentials(Arc::new(RwLock::new(None))), Default::default(), Arc::new(UnusedCpu));
        (root, executor, state, hash)
    }

    #[derive(Clone, Debug)]
    struct Persistence;
    impl SessionPersistence for Persistence {
        fn load_account_session(&self) -> io::Result<Option<Session>> {
            Ok(None)
        }
        fn save_account_session(&self, _: &Session) -> io::Result<()> {
            Ok(())
        }
        fn clear_account_session(&self) -> io::Result<()> {
            Ok(())
        }
    }

    fn session(user: &str, token: char) -> Session {
        Session::try_new("https://example.com".parse().unwrap(), user.into(), format!("{user}@example.com"), token.to_string().repeat(43), i64::MAX - 1, "r".repeat(43), i64::MAX).unwrap()
    }

    fn install_account(executor: &mut AssetExecutor) -> LibraryAccountSession<Persistence> {
        let account = LibraryAccountSession::load(Persistence, "https://example.com".parse().unwrap()).unwrap();
        account.set(session("reader", 'a')).unwrap();
        executor.authentication = LibraryAuthentication::Account(account.library_account());
        account
    }

    #[tokio::test]
    async fn transfer_preflight_rejects_missing_credentials_and_leaves_other_results_unchanged() {
        let (_root, mut executor, _state, _hash) = fixture(TransferOrigin::Background).await;
        let calls = Cell::new(0);
        let error = executor
            .refreshed(|| {
                calls.set(calls.get() + 1);
                std::future::ready(Ok(()))
            })
            .await
            .unwrap_err();
        assert!(matches!(error, TransferError::AuthenticationRequired));
        assert_eq!(calls.get(), 0);

        executor.authentication = LibraryAuthentication::Credentials(Arc::new(RwLock::new(Some(sync_transport::SyncCredentials::new("https://example.com".parse().unwrap(), "fixture-token")))));
        for expected in [Ok(()), Err(TransferError::retryable("temporary failure")), Err(TransferError::rejected("permanent failure")), Err(TransferError::AuthenticationRequired)] {
            calls.set(0);
            let result = executor
                .refreshed(|| {
                    calls.set(calls.get() + 1);
                    std::future::ready(expected.clone())
                })
                .await;
            assert_eq!(calls.get(), 1, "a transfer without account renewal cannot retry");
            assert_eq!(result.as_ref().err().map(ToString::to_string), expected.as_ref().err().map(ToString::to_string));
        }
    }

    #[tokio::test]
    async fn concurrent_token_renewal_retries_once_and_keeps_the_renewed_session() {
        for succeeds in [true, false] {
            let (_root, mut executor, _state, _hash) = fixture(TransferOrigin::Background).await;
            let account = install_account(&mut executor);
            let calls = Cell::new(0);
            let result = executor
                .refreshed(|| async {
                    calls.set(calls.get() + 1);
                    if calls.get() == 1 {
                        // Another worker renewed the token while the first request ran.
                        account.set(session("reader", 'b')).unwrap();
                        return Err(TransferError::AuthenticationRequired);
                    }
                    assert_eq!(account.require().unwrap().token(), "b".repeat(43));
                    if succeeds {
                        Ok(42)
                    } else {
                        Err(TransferError::AuthenticationRequired)
                    }
                })
                .await;
            assert_eq!(calls.get(), 2);
            if succeeds {
                assert_eq!(result.unwrap(), 42);
            } else {
                assert!(matches!(result, Err(TransferError::AuthenticationRequired)));
            }
            assert_eq!(account.require().unwrap().token(), "b".repeat(43), "a second rejection must not start another renewal");
        }
    }

    #[tokio::test]
    async fn retry_cannot_adopt_an_account_that_signed_in_during_the_transfer() {
        let (_root, mut executor, _state, _hash) = fixture(TransferOrigin::Background).await;
        let account = install_account(&mut executor);
        let calls = Cell::new(0);
        let result: Result<(), TransferError> = executor
            .refreshed(|| async {
                calls.set(calls.get() + 1);
                account.set(session("other-reader", 'b')).unwrap();
                Err(TransferError::AuthenticationRequired)
            })
            .await;
        assert!(matches!(result, Err(TransferError::AuthenticationRequired)));
        assert_eq!(calls.get(), 1);
        let current = account.require().unwrap();
        assert_eq!(current.user_id(), "other-reader");
        assert_eq!(current.token(), "b".repeat(43));
    }

    #[tokio::test]
    async fn an_unrefreshable_rejected_session_stops_before_a_second_transfer() {
        let (_root, mut executor, _state, _hash) = fixture(TransferOrigin::Background).await;
        let account = install_account(&mut executor);
        let calls = Cell::new(0);
        let result: Result<(), TransferError> = executor
            .refreshed(|| async {
                calls.set(calls.get() + 1);
                Err(TransferError::AuthenticationRequired)
            })
            .await;
        assert!(matches!(result, Err(TransferError::AuthenticationRequired)));
        assert_eq!(calls.get(), 1);
        assert!(account.session().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn user_storage_failures_alert_and_allow_an_explicit_retry() {
        for kind in [ErrorKind::StorageFull, ErrorKind::PermissionDenied] {
            let (_root, executor, mut state, hash) = fixture(TransferOrigin::UserInitiated).await;
            let updates = state.updates.subscribe();
            let error = TransferError::retryable(Error::from(kind));
            executor.report_download_failure(hash, TransferOrigin::UserInitiated, &error);
            state.drain(&executor.events);
            assert_eq!(updates.try_recv().unwrap(), LibraryUpdate::DownloadFailed { book_id: hash, failure: error.failure() });
            assert!(error.failure().is_some(), "storage failures are classified");
            assert_eq!(executor.state.read().await.database().book_download_origin(&hash).unwrap(), None);
            assert_eq!(state.updates.download_state(&hash), Some(DownloadState::NotDownloaded));
            assert!(executor.state.read().await.database().remote_asset_hashes(BlobKind::Book).unwrap().contains(&hash), "a local failure must preserve server presence");
            executor.state.read().await.database().sync_request_book_downloads(&[hash], TransferOrigin::UserInitiated).await.unwrap();
            assert_eq!(executor.state.read().await.database().book_download_origin(&hash).unwrap(), Some(TransferOrigin::UserInitiated));
        }
    }

    #[tokio::test]
    async fn a_mixed_manifest_skips_owned_bytes_and_preserves_admission_failures() {
        use sync_common::api::assets::{BlobManifestRejectionReason, BlobManifestResponse, RejectedBlobManifestEntry};

        let (_root, mut executor, _state, owned) = fixture(TransferOrigin::Background).await;
        let rejected = BookId::from_bytes([8; 32]);
        let canonical = ContentHash::from_bytes([9; 32]);
        let (credentials, server) = asset_server_with_head(
            BlobManifestResponse { owned: vec![owned], upload: Vec::new(), rejected: vec![RejectedBlobManifestEntry { book_id: rejected, reason: BlobManifestRejectionReason::QuotaExceeded }] },
            Some((owned, canonical, 6)),
        );
        executor.authentication = LibraryAuthentication::Credentials(Arc::new(RwLock::new(Some(credentials))));
        let results = executor.upload_books(&[BookUploadIntent { byte_checksum: None, id: 1, book_id: owned, size_bytes: 4 }, BookUploadIntent { byte_checksum: None, id: 2, book_id: rejected, size_bytes: 4 }]).await.unwrap();
        server.join().unwrap();

        // Neither book has local bytes. HEAD confirms the actual server
        // revision without uploading; rejection retains the quota cause.
        assert_eq!(executor.state.read().await.database().canonical_content_hash(owned).unwrap(), Some(canonical));
        assert_eq!(results.len(), 2);
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert!(results.iter().any(|result| matches!(result, Err(TransferError::Rejected(error)) if error.failure() == Some(client_protocol::Failure::CloudStorageFull))));
    }

    fn asset_server(response: impl serde::Serialize) -> (sync_transport::SyncCredentials, std::thread::JoinHandle<()>) {
        asset_server_with_head(response, None)
    }

    fn asset_server_with_head(response: impl serde::Serialize, head: Option<(BookId, ContentHash, u64)>) -> (sync_transport::SyncCredentials, std::thread::JoinHandle<()>) {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;
        use sync_common::transport;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap()).parse().unwrap();
        let bytes = transport::encode(&response).unwrap();
        let server = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut socket = BufReader::new(socket);
            let mut length = 0;
            let mut line = String::new();
            socket.read_line(&mut line).unwrap();
            assert!(line.starts_with("POST /api/libraries/"));
            loop {
                line.clear();
                socket.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            socket.read_exact(&mut vec![0; length]).unwrap();
            write!(socket.get_mut(), "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).unwrap();
            socket.get_mut().write_all(&bytes).unwrap();
            drop(socket);
            if let Some((book, checksum, size)) = head {
                let (socket, _) = listener.accept().unwrap();
                socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let mut socket = BufReader::new(socket);
                line.clear();
                socket.read_line(&mut line).unwrap();
                assert!(line.starts_with("HEAD /api/libraries/") && line.contains(&format!("/blobs/{book} ")));
                loop {
                    line.clear();
                    socket.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                }
                write!(socket.get_mut(), "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nx-bokheim-content-hash: {checksum}\r\nConnection: close\r\n\r\n").unwrap();
            }
        });
        (sync_transport::SyncCredentials::new(origin, "test-token"), server)
    }

    #[tokio::test]
    async fn cover_presence_preserves_missing_request_order_and_updates_only_presence() {
        use sync_common::api::assets::ThumbnailPresenceResponse;

        for (answer, previously_present) in [(vec![], true), (vec![0], false), (vec![1], true), (vec![1, 0], false)] {
            let (_root, mut executor, _state, first) = fixture(TransferOrigin::Background).await;
            let second = BookId::from_bytes([8; 32]);
            let third = BookId::from_bytes([9; 32]);
            let hashes = [first, second, third];
            let request = [second, first, third, second, first];
            if previously_present {
                executor.state.read().await.database().record_remote_asset(BlobKind::Thumbnail, &first).await.unwrap();
            }
            let work = executor.state.read().await.database().thumbnail_work_state(&first).await.unwrap();
            let present = answer.into_iter().map(|index| hashes[index]).collect::<Vec<_>>();
            let expected = request.iter().filter(|hash| !present.contains(hash)).copied().collect::<Vec<_>>();
            let (credentials, server) = asset_server(ThumbnailPresenceResponse { present: present.clone(), revisions: vec![] });
            executor.authentication = LibraryAuthentication::Credentials(Arc::new(RwLock::new(Some(credentials))));
            assert_eq!(executor.cover_presence(&request).await.unwrap(), expected);
            server.join().unwrap();
            assert_eq!(executor.state.read().await.database().server_has_cover(&first).unwrap(), present.contains(&first));
            assert!(executor.state.read().await.database().remote_asset_hashes(BlobKind::Book).unwrap().contains(&first));
            assert_eq!(executor.state.read().await.database().book_download_origin(&first).unwrap(), Some(TransferOrigin::Background));
            assert_eq!(executor.state.read().await.database().thumbnail_work_state(&first).unwrap(), work);
        }
    }

    #[tokio::test]
    async fn invalid_cover_presence_responses_leave_all_presence_facts_unchanged() {
        use sync_common::api::assets::ThumbnailPresenceResponse;

        for previously_present in [false, true] {
            for duplicate in [false, true] {
                let (_root, mut executor, _state, hash) = fixture(TransferOrigin::Background).await;
                if previously_present {
                    executor.state.read().await.database().record_remote_asset(BlobKind::Thumbnail, &hash).await.unwrap();
                }
                let second = BookId::from_bytes([8; 32]);
                let unexpected = BookId::from_bytes([9; 32]);
                let present = vec![hash, if duplicate { hash } else { unexpected }];
                let (credentials, server) = asset_server(ThumbnailPresenceResponse { present, revisions: vec![] });
                executor.authentication = LibraryAuthentication::Credentials(Arc::new(RwLock::new(Some(credentials))));
                let result = executor.cover_presence(&[hash, second, hash]).await;
                server.join().unwrap();
                assert!(matches!(result, Err(TransferError::Retryable(ref error)) if error.to_string().contains("invalid thumbnail presence response")));
                assert_eq!(executor.state.read().await.database().server_has_cover(&hash).unwrap(), previously_present);
                assert!(executor.state.read().await.database().remote_asset_hashes(BlobKind::Book).unwrap().contains(&hash));
                assert_eq!(executor.state.read().await.database().book_download_origin(&hash).unwrap(), Some(TransferOrigin::Background));
            }
        }
    }

    #[tokio::test]
    async fn server_thumbnail_download_preserves_both_images_without_cpu_work() {
        use library_files::asset_store::thumbnail_variant_name;

        // Cover both a fresh download and an older cache with only a master.
        for cached_master in [false, true] {
            let (_root, mut executor, _state, hash) = fixture(TransferOrigin::Background).await;
            if cached_master {
                executor.state.read().await.assets().write(BlobKind::Thumbnail, &hash, b"\xff\xd8\xffold master").await.unwrap();
            }
            let master = b"\xff\xd8\xffserver master";
            let browse = b"\xff\xd8\xffserver browse";
            let (credentials, server) = asset_server(ThumbnailBatchDownloadResponse { thumbnails: vec![ThumbnailBatchEntry { book_id: hash, bytes: master.to_vec(), browse_bytes: browse.to_vec() }], missing: vec![] });
            executor.authentication = LibraryAuthentication::Credentials(Arc::new(RwLock::new(Some(credentials))));
            executor.download_thumbnail_batch(&[hash]).await.unwrap();
            server.join().unwrap();
            assert_eq!(executor.state.read().await.assets().read_bytes(BlobKind::Thumbnail, &hash).await.unwrap().unwrap(), master);
            assert_eq!(executor.state.read().await.assets().read_named(thumbnail_variant_name(&hash, thumbnail::BROWSE_THUMBNAIL_WIDTH)).await.unwrap().unwrap(), browse);
            assert_eq!(executor.state.read().await.assets().read_named(thumbnail_variant_name(&hash, thumbnail::THUMBNAIL_WIDTH)).await.unwrap().unwrap(), master);
            assert_eq!(executor.state.read().await.database().thumbnail_work_state(&hash).unwrap().as_deref(), Some("ready"));
            // The server has stopped: a cached pair must require no further request.
            executor.download_thumbnail_batch(&[hash]).await.unwrap();
        }
    }

    #[tokio::test]
    async fn invalid_server_browse_thumbnail_is_rejected_before_storage() {
        let (_root, mut executor, _state, hash) = fixture(TransferOrigin::Background).await;
        let (credentials, server) = asset_server(ThumbnailBatchDownloadResponse { thumbnails: vec![ThumbnailBatchEntry { book_id: hash, bytes: b"\xff\xd8\xffmaster".to_vec(), browse_bytes: b"not a JPEG".to_vec() }], missing: vec![] });
        executor.authentication = LibraryAuthentication::Credentials(Arc::new(RwLock::new(Some(credentials))));
        assert!(executor.download_thumbnail_batch(&[hash]).await.is_err());
        server.join().unwrap();
        assert!(!executor.state.read().await.assets().exists(BlobKind::Thumbnail, &hash).await.unwrap());
        assert!(!executor.state.read().await.assets().thumbnail_set_exists(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn malformed_thumbnail_responses_preserve_completed_covers() {
        for invalid in ["duplicate entry", "unrequested entry", "entry also missing", "duplicate missing", "unrequested missing", "omitted"] {
            let (_root, mut executor, mut state, hash) = fixture(TransferOrigin::Background).await;
            let updates = state.updates.subscribe();
            let second = BookId::from_bytes([8; 32]);
            let unknown = BookId::from_bytes([9; 32]);
            let first = ThumbnailBatchEntry { book_id: hash, bytes: b"\xff\xd8\xffmaster".to_vec(), browse_bytes: b"\xff\xd8\xffbrowse".to_vec() };
            let mut response = ThumbnailBatchDownloadResponse { thumbnails: vec![first.clone()], missing: vec![] };
            match invalid {
                "duplicate entry" => {
                    let mut repeated = first.clone();
                    repeated.bytes.extend_from_slice(b"replacement");
                    response.thumbnails.push(repeated);
                }
                "unrequested entry" => response.thumbnails.push(ThumbnailBatchEntry { book_id: unknown, ..first.clone() }),
                "entry also missing" => response.missing.push(hash),
                "duplicate missing" => response.missing.extend([second, second]),
                "unrequested missing" => response.missing.push(unknown),
                "omitted" => {}
                _ => unreachable!(),
            }
            let (credentials, server) = asset_server(response);
            executor.authentication = LibraryAuthentication::Credentials(Arc::new(RwLock::new(Some(credentials))));
            let result = executor.download_thumbnail_batch(&[hash, second]).await;
            server.join().unwrap();
            assert!(matches!(result, Err(TransferError::Retryable(_))), "{invalid}: {result:?}");
            assert_eq!(executor.state.read().await.assets().read_bytes(BlobKind::Thumbnail, &hash).await.unwrap(), Some(first.bytes), "{invalid}");
            assert!(executor.state.read().await.assets().thumbnail_set_exists(&hash).await.unwrap());
            assert!(!executor.state.read().await.assets().thumbnail_set_exists(&unknown).await.unwrap());
            assert!(executor.state.read().await.database().server_has_cover(&hash).unwrap(), "{invalid}");
            assert_eq!(executor.state.read().await.database().thumbnail_work_state(&hash).unwrap().as_deref(), Some("ready"));
            state.drain(&executor.events);
            assert_eq!(updates.try_recv().unwrap(), LibraryUpdate::Covers(vec![hash]));
            assert!(updates.is_empty());
        }
    }

    #[tokio::test]
    async fn thumbnail_responses_accept_missing_covers_and_deduplicate_requests() {
        for with_cover in [false, true] {
            let (_root, mut executor, _state, hash) = fixture(TransferOrigin::Background).await;
            let second = BookId::from_bytes([8; 32]);
            for hash in [hash, second] {
                executor.state.read().await.database().record_remote_asset(BlobKind::Thumbnail, &hash).await.unwrap();
            }
            let response = ThumbnailBatchDownloadResponse {
                thumbnails: if with_cover { vec![ThumbnailBatchEntry { book_id: hash, bytes: b"\xff\xd8\xffmaster".to_vec(), browse_bytes: b"\xff\xd8\xffbrowse".to_vec() }] } else { vec![] },
                missing: if with_cover { vec![second] } else { vec![hash, second] },
            };
            let (credentials, server) = asset_server(response);
            executor.authentication = LibraryAuthentication::Credentials(Arc::new(RwLock::new(Some(credentials))));
            executor.download_thumbnail_batch(&[hash, hash, second]).await.unwrap();
            server.join().unwrap();
            assert_eq!(executor.state.read().await.database().server_has_cover(&hash).unwrap(), with_cover);
            assert_eq!(executor.state.read().await.assets().thumbnail_set_exists(&hash).await.unwrap(), with_cover);
            assert!(!executor.state.read().await.database().server_has_cover(&second).unwrap());
        }
    }

    struct RecoveryProcessor {
        cached: bool,
        has_cover: bool,
        fails: bool,
    }

    impl RecoveryProcessor {
        fn versions() -> ThumbnailVersions {
            ThumbnailVersions { browse: b"browse".to_vec(), card_high_density: b"card".to_vec(), high_density: b"master".to_vec() }
        }
    }

    impl crate::thumbnail_jobs::ThumbnailProcessor for RecoveryProcessor {
        fn decode(&self, bytes: Vec<u8>) -> BoxedBackendFuture<'_, Result<ThumbnailVersions, BackendError>> {
            assert!(self.cached, "only the cached-artwork case should decode an image");
            assert_eq!(bytes, b"cached");
            Box::pin(async { Ok(Self::versions()) })
        }
        fn generate(&self, extension: String, mut reader: book_access::BoxedBookReader) -> BoxedBackendFuture<'_, Result<Option<ThumbnailVersions>, BackendError>> {
            assert!(!self.cached, "cached artwork should not reopen the book");
            assert_eq!(extension, "epub");
            Box::pin(async move {
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes).unwrap();
                assert_eq!(bytes, b"book", "generation must read the book from its beginning");
                if self.fails {
                    return Err(BackendError::message("CPU work failed"));
                }
                Ok(self.has_cover.then(Self::versions))
            })
        }
    }

    #[derive(Default)]
    struct RecoveryObserver {
        completed: usize,
        ready: Vec<BookId>,
    }
    impl crate::thumbnail_jobs::ThumbnailObserver for RecoveryObserver {
        fn completed(&mut self) {
            self.completed += 1;
        }
        fn ready(&mut self, hash: BookId) {
            self.ready.push(hash);
        }
    }

    #[tokio::test]
    async fn thumbnail_recovery_completes_cached_generated_and_absent_artwork() {
        use crate::thumbnail_jobs::ThumbnailJobs;
        use library_files::asset_store::thumbnail_variant_name;

        for (cached, has_cover) in [(true, true), (false, true), (false, false)] {
            let (root, executor, _state, hash) = fixture(TransferOrigin::Background).await;
            executor.state.read().await.database().shared_request_thumbnail(&hash).await.unwrap();
            if cached {
                executor.state.read().await.assets().write(BlobKind::Thumbnail, &hash, b"cached").await.unwrap();
            } else {
                std::fs::write(root.path().join("book.epub"), b"book").unwrap();
            }
            let processor = RecoveryProcessor { cached, has_cover, fails: false };
            let mut observer = RecoveryObserver::default();
            ThumbnailJobs::new(&executor.state.read().await.database(), &executor.state.read().await.assets(), &processor).run(vec![hash], &mut observer).await.unwrap();
            assert_eq!(observer.completed, 1);
            assert_eq!(observer.ready, if has_cover { vec![hash] } else { vec![] });
            assert_eq!(executor.state.read().await.database().thumbnail_work_state(&hash).unwrap().as_deref(), Some(if has_cover { "ready" } else { "no_cover" }));
            assert_eq!(executor.state.read().await.assets().thumbnail_set_exists(&hash).await.unwrap(), has_cover);
            if has_cover {
                assert_eq!(executor.state.read().await.assets().read_bytes(BlobKind::Thumbnail, &hash).await.unwrap().unwrap(), b"master");
                assert_eq!(executor.state.read().await.assets().read_named(thumbnail_variant_name(&hash, thumbnail::BROWSE_THUMBNAIL_WIDTH)).await.unwrap().unwrap(), b"browse");
                assert_eq!(executor.state.read().await.assets().read_named(thumbnail_variant_name(&hash, thumbnail::THUMBNAIL_WIDTH)).await.unwrap().unwrap(), b"card");
            }
        }
    }

    #[tokio::test]
    async fn thumbnail_recovery_keeps_failed_work_pending_and_can_retry() {
        use crate::thumbnail_jobs::ThumbnailJobs;

        let (root, executor, _state, hash) = fixture(TransferOrigin::Background).await;
        executor.state.read().await.database().shared_request_thumbnail(&hash).await.unwrap();
        std::fs::write(root.path().join("book.epub"), b"book").unwrap();
        let processor = RecoveryProcessor { cached: false, has_cover: true, fails: true };
        let mut observer = RecoveryObserver::default();
        let error = ThumbnailJobs::new(&executor.state.read().await.database(), &executor.state.read().await.assets(), &processor).run(vec![hash], &mut observer).await.unwrap_err();
        assert!(error.to_string().contains("CPU work failed"));
        assert_eq!(executor.state.read().await.database().pending_thumbnail_job_count().unwrap(), 1);
        assert!(!executor.state.read().await.assets().thumbnail_set_exists(&hash).await.unwrap());
        assert_eq!(observer.completed, 0);
        assert!(observer.ready.is_empty());

        let processor = RecoveryProcessor { fails: false, ..processor };
        ThumbnailJobs::new(&executor.state.read().await.database(), &executor.state.read().await.assets(), &processor).run(vec![hash], &mut observer).await.unwrap();
        assert_eq!(executor.state.read().await.database().pending_thumbnail_job_count().unwrap(), 0);
        assert_eq!(executor.state.read().await.database().thumbnail_work_state(&hash).unwrap().as_deref(), Some("ready"));
        assert_eq!(observer.completed, 1);
        assert_eq!(observer.ready, vec![hash]);
    }

    #[tokio::test]
    async fn local_recovery_leaves_server_master_untouched() {
        use crate::thumbnail_jobs::{ThumbnailExtractor, ThumbnailJobs, ThumbnailObserver};

        struct Observer;
        impl ThumbnailObserver for Observer {
            fn completed(&mut self) {}
            fn ready(&mut self, _: BookId) {
                panic!("server cover must be downloaded, not generated");
            }
        }

        let (_root, executor, _state, hash) = fixture(TransferOrigin::Background).await;
        let master = b"\xff\xd8\xffcached server master";
        executor.state.read().await.assets().write(BlobKind::Thumbnail, &hash, master).await.unwrap();
        executor.state.read().await.database().record_remote_asset(BlobKind::Thumbnail, &hash).await.unwrap();
        executor.state.read().await.database().shared_request_thumbnail(&hash).await.unwrap();
        let processor = ThumbnailExtractor::new(executor.cpu.clone());
        let jobs = ThumbnailJobs::new(&executor.state.read().await.database(), &executor.state.read().await.assets(), &processor);
        jobs.run(vec![hash], &mut Observer).await.unwrap();
        assert_eq!(executor.state.read().await.assets().read_bytes(BlobKind::Thumbnail, &hash).await.unwrap().unwrap(), master);
    }

    #[tokio::test]
    async fn background_storage_failures_remain_queued_without_user_alerts() {
        for kind in [ErrorKind::StorageFull, ErrorKind::PermissionDenied] {
            let (_root, executor, mut state, hash) = fixture(TransferOrigin::Background).await;
            let updates = state.updates.subscribe();
            executor.report_download_failure(hash, TransferOrigin::Background, &TransferError::retryable(Error::from(kind)));
            state.drain(&executor.events);
            assert_eq!(executor.state.read().await.database().book_download_origin(&hash).unwrap(), Some(TransferOrigin::Background));
            assert_eq!(state.updates.download_state(&hash), Some(DownloadState::Queued));
            assert!(!std::iter::from_fn(|| updates.try_recv().ok()).any(|update| matches!(update, LibraryUpdate::DownloadFailed { .. })));
        }
    }

    #[tokio::test]
    async fn a_failure_before_download_bytes_arrive_also_reaches_the_user() {
        let (_root, executor, mut state, hash) = fixture(TransferOrigin::UserInitiated).await;
        let updates = state.updates.subscribe();
        assert!(matches!(executor.download_book(hash).await, Err(TransferError::AuthenticationRequired)));
        state.drain(&executor.events);
        assert!(matches!(updates.try_recv().unwrap(), LibraryUpdate::DownloadFailed { book_id, .. } if book_id == hash));
        assert_eq!(executor.state.read().await.database().book_download_origin(&hash).unwrap(), None);
    }

    #[tokio::test]
    async fn actual_download_space_check_alerts_only_for_user_actions() {
        use std::time::Duration;
        use sync_transport::SyncCredentials;
        use tokio::{
            io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
            net::TcpListener,
            time::timeout,
        };

        for origin in [TransferOrigin::UserInitiated, TransferOrigin::Background] {
            let (_root, mut executor, mut state, hash) = fixture(origin).await;
            let updates = state.updates.subscribe();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let server_url = format!("http://{}", listener.local_addr().unwrap()).parse().unwrap();
            executor.authentication = LibraryAuthentication::Credentials(Arc::new(RwLock::new(Some(SyncCredentials::new(server_url, "test-token")))));
            let server = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let mut socket = BufReader::new(socket);
                let mut line = String::new();
                loop {
                    line.clear();
                    if socket.read_line(&mut line).await.unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                }
                // Advertise more bytes than any drive can hold. No disk needs
                // to be filled and no response body needs to be downloaded.
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 9223372036854775807\r\nConnection: close\r\n\r\n").await.unwrap();
            });
            let error = timeout(Duration::from_secs(5), executor.download_book(hash)).await.unwrap().unwrap_err();
            server.await.unwrap();
            assert_eq!(error.failure(), Some(client_protocol::Failure::StorageFull), "{error}");
            state.drain(&executor.events);
            let alerted = std::iter::from_fn(|| updates.try_recv().ok()).any(|update| matches!(update, LibraryUpdate::DownloadFailed { .. }));
            assert_eq!(alerted, origin == TransferOrigin::UserInitiated);
            assert_eq!(executor.state.read().await.database().book_download_origin(&hash).unwrap(), if origin == TransferOrigin::Background { Some(origin) } else { None });
            assert!(executor.state.read().await.database().remote_asset_hashes(BlobKind::Book).unwrap().contains(&hash));
        }
    }
}
