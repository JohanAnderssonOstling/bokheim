#[cfg(test)]
use super::asset_sync::should_download_thumbnail;
use super::asset_sync::{AssetSyncConfig, AssetSyncEngine};
#[cfg(test)]
use super::error::SyncError;
use super::error::SyncResult;
use super::state_sync::{StateSyncConfig, StateSyncEngine};
#[cfg(test)]
use super::state_sync::{StateSyncResult, CURSOR_RECOVERY_PENDING_KEY};
#[cfg(test)]
use super::transfer::TransferJob;
use super::transfer::{TransferOrigin, TransferQueue, TransferRuntime, TransferWorker, TransferWorkerConfig};
use crate::library::database as db;
use crate::platform::database::Database;
use crate::platform::library_events::{LibraryEvent, LibraryEventSender};
#[cfg(test)]
use crate::BlobKind;
#[cfg(test)]
use crate::RelativeDirPath;
use crate::{ContentHash, ServerUrl};
#[cfg(test)]
use library_replica::coalesce_mutations_for_push;
#[cfg(test)]
use library_replica::{mutations_ready_for_push, prepare_push};
use std::collections::HashSet;
#[cfg(test)]
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
#[cfg(test)]
use sync_client::{validate_pull_batch, validate_push_response, PushResponseError};
#[cfg(test)]
use sync_common::api::assets::BlobManifestEntry;
#[cfg(test)]
use sync_common::SyncExchangeResponse;
#[cfg(test)]
use sync_common::{DirId, PullStateResponse, PushMutationsResponse};
use sync_common::{LibraryId, ReplicaId};

const AUTH_EXPIRY_SAFETY_WINDOW_SECONDS: i64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AssetSync {
    Skip,
    Transfer(TransferOrigin),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReadingPositionSync {
    #[cfg(test)]
    Immediate,
    AfterQuietPeriod(Duration),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SyncPolicy {
    pub assets: AssetSync,
    pub reading_positions: ReadingPositionSync,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SyncOutcome {
    pub next_reading_upload_in: Option<Duration>,
    pub mutation_issues: Vec<sync_common::MutationIssue>,
}

pub(crate) struct SyncManagerConfig {
    pub(crate) database: Database,
    pub(crate) replica_id: ReplicaId,
    pub(crate) server_url: ServerUrl,
    pub(crate) library_id: LibraryId,
    pub(crate) event_tx: Option<LibraryEventSender>,
    pub(crate) transfer_runtime: TransferRuntime,
    pub(crate) pending_thumbnail_jobs: Arc<std::sync::Mutex<HashSet<ContentHash>>>,
}

#[derive(Debug)]
pub struct SyncManager {
    database: Database,
    replica_id: ReplicaId,
    server_url: ServerUrl,
    http_client: reqwest::Client,
    library_id: LibraryId,
    event_tx: Option<LibraryEventSender>,
    session: Arc<std::sync::RwLock<Option<sync_client::Session>>>,
    transfer_runtime: TransferRuntime,
    transfer_queue: Arc<TransferQueue>,
    pending_thumbnail_jobs: Arc<std::sync::Mutex<HashSet<ContentHash>>>,
    sync_gate: tokio::sync::Mutex<()>,
    inventory_checked: AtomicBool,
}

impl SyncManager {
    pub(crate) fn new(config: SyncManagerConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let http_client = reqwest::Client::new();
        let transfer_queue = Arc::new(TransferQueue::with_history_db(config.database.clone()));
        Ok(Self {
            database: config.database,
            replica_id: config.replica_id,
            server_url: config.server_url,
            http_client,
            library_id: config.library_id,
            event_tx: config.event_tx,
            session: Arc::new(std::sync::RwLock::new(None)),
            transfer_runtime: config.transfer_runtime,
            transfer_queue,
            pending_thumbnail_jobs: config.pending_thumbnail_jobs,
            sync_gate: tokio::sync::Mutex::new(()),
            inventory_checked: AtomicBool::new(false),
        })
    }

    /// The server-issued user id for the active session, or None when signed out.
    pub fn user_id(&self) -> Option<String> {
        self.session.read().ok().and_then(|guard| guard.as_ref().map(|session| session.user_id().to_owned()))
    }

    pub fn is_signed_in(&self) -> bool {
        self.session.read().map(|s| s.is_some()).unwrap_or(false)
    }

    pub fn current_session(&self) -> Option<sync_client::Session> {
        self.session.read().ok().and_then(|s| s.clone())
    }

    pub fn bound_user_id(&self) -> Result<Option<String>, String> {
        let conn = self.database.get().map_err(|error| error.to_string())?;
        db::get_sync_metadata(&conn, "bound_user_id").map_err(|error| error.to_string())
    }

    /// Binds this local library to the session's server-issued user. Switching
    /// accounts clears account-specific remote presence and pull-cursor state,
    /// while retaining the local library itself.
    pub fn adopt_session(&self, session: sync_client::Session) -> Result<(), String> {
        if session.server_url() != &self.server_url {
            return Err(format!("session belongs to {}, but this library syncs with {}", session.server_url(), self.server_url));
        }
        let mut rebound = false;
        {
            let conn = self.database.get().map_err(|e| e.to_string())?;
            match db::get_sync_metadata(&conn, "bound_user_id").map_err(|error| error.to_string())? {
                Some(bound) if bound != session.user_id() => {
                    db::rebind_sync_account(&conn, session.user_id()).map_err(|error| error.to_string())?;
                    rebound = true;
                }
                Some(_) => {}
                None => db::set_sync_metadata(&conn, "bound_user_id", session.user_id()).map_err(|error| error.to_string())?,
            }
        }
        if rebound {
            self.inventory_checked.store(false, Ordering::Release);
        }
        let mut guard = self.session.write().map_err(|_| "session lock is poisoned".to_string())?;
        *guard = Some(session);
        Ok(())
    }

    pub fn clear_session(&self) -> Result<(), String> {
        {
            let mut guard = self.session.write().map_err(|_| "session lock is poisoned".to_string())?;
            *guard = None;
        }
        Ok(())
    }

    pub(crate) async fn synchronize(&self, policy: SyncPolicy) -> SyncResult<SyncOutcome> {
        let _sync_guard = self.sync_gate.lock().await;
        if !self.is_signed_in() {
            return Ok(SyncOutcome::default());
        }
        if self.current_session().is_some_and(|session| if session.refresh_token().is_some() { session.refresh_expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) } else { session.expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) }) {
            self.clear_session().map_err(std::io::Error::other)?;
            log::warn!("Authentication credentials expired or are too close to expiry; sign in again before synchronizing");
            return Ok(SyncOutcome::default());
        }
        let reading_debounce = match policy.reading_positions {
            #[cfg(test)]
            ReadingPositionSync::Immediate => None,
            ReadingPositionSync::AfterQuietPeriod(duration) => Some(duration),
        };
        let state = self.state_engine().synchronize(reading_debounce).await?;
        if let AssetSync::Transfer(origin) = policy.assets {
            self.sync_assets(origin).await?;
        } else if state.assets_needed {
            self.sync_assets(TransferOrigin::Background).await?;
        }
        let completed_at = u64::try_from(web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH)?.as_millis())?;
        let conn = self.database.get()?;
        db::set_sync_metadata(&conn, "last_successful_sync_at", &completed_at.to_string())?;
        Ok(SyncOutcome { next_reading_upload_in: state.next_reading_upload_in, mutation_issues: state.mutation_issues })
    }

    #[cfg(test)]
    async fn sync_once(&self) -> SyncResult<()> {
        self.synchronize(SyncPolicy { assets: AssetSync::Transfer(TransferOrigin::Background), reading_positions: ReadingPositionSync::Immediate }).await.map(|_| ())
    }

    fn state_engine(&self) -> StateSyncEngine<'_> {
        StateSyncEngine::new(StateSyncConfig { db_pool: &self.database, http_client: &self.http_client, library_id: self.library_id, replica_id: self.replica_id, session: self.session.as_ref(), inventory_checked: &self.inventory_checked })
    }

    #[cfg(test)]
    async fn sync_state_under_gate(&self, reading_debounce: Option<Duration>) -> SyncResult<StateSyncResult> {
        self.state_engine().synchronize(reading_debounce).await
    }

    #[cfg(test)]
    fn begin_cursor_recovery(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.state_engine().begin_cursor_recovery()
    }

    pub async fn resume_transfers(&self) -> SyncResult<()> {
        if !self.is_signed_in() {
            return Ok(());
        }
        self.transfer_worker().run_pending(4).await.map_err(Into::into)
    }

    fn transfer_worker(&self) -> TransferWorker {
        TransferWorker::new(TransferWorkerConfig {
            http_client: self.http_client.clone(),
            library_id: self.library_id,
            db_pool: self.database.clone(),
            event_tx: self.event_tx.clone(),
            session: Arc::clone(&self.session),
            runtime: self.transfer_runtime.clone(),
            queue: Arc::clone(&self.transfer_queue),
        })
    }

    fn asset_engine(&self) -> AssetSyncEngine<'_> {
        AssetSyncEngine::new(AssetSyncConfig {
            db_pool: &self.database,
            http_client: &self.http_client,
            library_id: self.library_id,
            session: self.session.as_ref(),
            pending_thumbnail_jobs: self.pending_thumbnail_jobs.as_ref(),
            transfer_worker: self.transfer_worker(),
        })
    }

    async fn sync_assets(&self, upload_origin: TransferOrigin) -> SyncResult<()> {
        self.asset_engine().synchronize(upload_origin).await
    }

    pub fn transfer_statuses(&self) -> Vec<crate::TransferStatus> {
        self.transfer_queue.statuses()
    }

    pub fn transfer_history(&self) -> Vec<crate::TransferStatus> {
        self.transfer_queue.history()
    }

    pub fn request_download(&self, content_hash: ContentHash) -> Result<(), Box<dyn std::error::Error>> {
        self.request_downloads(std::slice::from_ref(&content_hash))
    }

    pub fn request_downloads(&self, content_hashes: &[ContentHash]) -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = self.database.get()?;
        db::request_book_downloads(&mut conn, content_hashes, TransferOrigin::UserInitiated)?;
        drop(conn);
        if let Some(tx) = &self.event_tx {
            for content_hash in content_hashes {
                let _ = tx.try_send(LibraryEvent::DownloadStatusChanged { content_hash: *content_hash, state: crate::DownloadState::Queued });
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn download_job(&self, conn: &rusqlite::Connection, content_hash: &ContentHash, origin: TransferOrigin) -> Result<Option<TransferJob>, rusqlite::Error> {
        self.asset_engine().download_job(conn, content_hash, origin)
    }

    #[cfg(test)]
    fn apply_push_response(&self, requested_ids: &[sync_common::MutationId], push_response: &PushMutationsResponse) -> Result<Vec<sync_common::MutationRejection>, Box<dyn std::error::Error>> {
        self.state_engine().apply_push_response(requested_ids, push_response)
    }

    #[cfg(test)]
    async fn exchange_events_with_server(&self, starting_cursor: sync_common::SyncCursor, reading_debounce: Option<Duration>) -> SyncResult<StateSyncResult> {
        self.state_engine().exchange_events_with_server(starting_cursor, reading_debounce).await
    }

    #[cfg(test)]
    async fn pull_events_from_server(&self, cursor: sync_common::SyncCursor) -> SyncResult<()> {
        self.state_engine().pull_events_from_server(cursor).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_model::{AnnotationAnchor, AnnotationState, AnnotationStyle, BookFormat, BookLifecycleState, BookMetadataState, ReadingPositionState};
    use library_replica::{MutationBody, StateMutation};
    use std::io::Write;
    use std::path::PathBuf;
    use sync_common::{LibraryRevision, MutationRejection, MutationRejectionReason, ReplicaSeq, ServerMutation, SyncCursor};
    use test_support::{fixture_content_hash, fixture_dir_id, fixture_library_id, fixture_replica_id};

    #[test]
    fn local_coverless_books_do_not_probe_remote_thumbnails() {
        assert!(!should_download_thumbnail(true, false, false, false));
        assert!(should_download_thumbnail(true, false, true, false));
        assert!(should_download_thumbnail(false, false, false, false));
        assert!(!should_download_thumbnail(false, true, true, false));
        assert!(!should_download_thumbnail(false, false, true, true));
    }

    const TEST_CONTENT_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const TEST_AUTH_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn test_content_hash() -> ContentHash {
        ContentHash::new(&TEST_CONTENT_HASH)
    }

    fn test_epub(payload: &[u8]) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let compressed = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        archive.start_file("mimetype", stored).unwrap();
        archive.write_all(b"application/epub+zip").unwrap();
        archive.start_file("META-INF/container.xml", compressed).unwrap();
        archive
            .write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)
            .unwrap();
        archive.start_file("OEBPS/content.opf", compressed).unwrap();
        archive.write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#).unwrap();
        archive.start_file("OEBPS/chapter.xhtml", compressed).unwrap();
        archive.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><pre>"#).unwrap();
        archive.write_all(payload).unwrap();
        archive.write_all(b"</pre></body></html>").unwrap();
        archive.finish().unwrap().into_inner()
    }

    fn local_seq(value: u64) -> ReplicaSeq {
        ReplicaSeq::new(value).unwrap()
    }

    fn millis(value: u64) -> sync_common::UnixMillis {
        value
    }

    fn pulled_change(server_sequence: u64, body: MutationBody) -> ServerMutation {
        ServerMutation {
            mutation: StateMutation { mutation_id: sync_common::MutationId::new(), body, changed_at: millis(10), replica_seq: local_seq(server_sequence) }.to_wire().unwrap(),
            replica_id: test_support::fixture_replica_id("remote-device"),
            revision: LibraryRevision::new(server_sequence).unwrap(),
        }
    }

    fn deleted(content_hash: u64) -> MutationBody {
        MutationBody::BookLifecycle { content_hash: fixture_content_hash(content_hash), value: BookLifecycleState::Deleted { origin_folder_id: None } }
    }

    fn added(content_hash: u64, title: &str, added_at: u64) -> MutationBody {
        MutationBody::BookLifecycle {
            content_hash: fixture_content_hash(content_hash),
            value: BookLifecycleState::Present {
                metadata: BookMetadataState { title: title.to_owned(), contributors: Vec::new(), description: String::new(), publication: Default::default() },
                added_at: millis(added_at),
                format: BookFormat::Epub,
            },
        }
    }

    fn cursor(revision: Option<LibraryRevision>) -> SyncCursor {
        SyncCursor { state_revision: revision, reading_revision: revision }
    }

    fn pull_response(mutations: Vec<ServerMutation>, next_revision: Option<LibraryRevision>, has_more: bool) -> PullStateResponse {
        let next_cursor = if has_more {
            let mut cursor = SyncCursor::default();
            for mutation in &mutations {
                if mutation.mutation.kind == sync_common::mutation_kind::READING_POSITION {
                    cursor.reading_revision = Some(mutation.revision);
                } else {
                    cursor.state_revision = Some(mutation.revision);
                }
            }
            cursor
        } else {
            cursor(next_revision)
        };
        PullStateResponse { mutations, next_cursor, has_more }
    }

    fn binary_sync_response<T: serde::Serialize>(value: &T) -> axum::response::Response {
        use axum::response::IntoResponse;

        let encoded = sync_common::transport::encode(value).unwrap();
        let mut response = encoded.into_response();
        response.headers_mut().insert(axum::http::header::CONTENT_TYPE, axum::http::HeaderValue::from_static(sync_common::transport::MEDIA_TYPE));
        response
    }

    fn decode_sync_exchange(headers: &axum::http::HeaderMap, body: &[u8]) -> sync_common::SyncExchangeRequest {
        assert!(headers.get(axum::http::header::CONTENT_ENCODING).is_none());
        sync_common::transport::decode(body, sync_common::transport::MAX_DECODED_REQUEST_BYTES).unwrap()
    }

    #[tokio::test]
    async fn opening_sync_republishes_state_missing_from_the_server_inventory() {
        #[derive(Clone)]
        struct InventoryState(Arc<std::sync::Mutex<Vec<String>>>);

        async fn inventory(body: axum::body::Bytes) -> axum::response::Response {
            let request: sync_common::StateInventoryRequest = sync_common::transport::decode(&body, sync_common::transport::MAX_DECODED_REQUEST_BYTES).unwrap();
            binary_sync_response(&sync_common::StateInventoryResponse { missing: request.cells })
        }

        async fn exchange(axum::extract::State(state): axum::extract::State<InventoryState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            let request = decode_sync_exchange(&headers, &body);
            state.0.lock().unwrap().extend(request.mutations.iter().map(|mutation| mutation.kind.clone()));
            binary_sync_response(&SyncExchangeResponse {
                push: PushMutationsResponse { accepted: request.mutations.iter().map(|mutation| mutation.mutation_id).collect(), rejected: Vec::new() },
                pull: PullStateResponse { mutations: Vec::new(), next_cursor: request.cursor, has_more: false },
            })
        }

        let pushed_kinds = Arc::new(std::sync::Mutex::new(Vec::new()));
        let app = axum::Router::new().route("/api/sync/inventory", axum::routing::post(inventory)).route("/api/sync/exchange", axum::routing::post(exchange)).with_state(InventoryState(Arc::clone(&pushed_kinds)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        *manager.session.write().unwrap() = Some(sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        {
            let conn = manager.database.get().unwrap();
            let dir_id = fixture_dir_id("inventory/shelf");
            let hash = fixture_content_hash(7);
            conn.execute("INSERT INTO dir(id,parent_id,name,intent_parent_id,intent_name,intent_lifecycle) VALUES(?1,'00000000-0000-0000-0000-000000000000','Shelf','00000000-0000-0000-0000-000000000000','Shelf',0)", [dir_id.to_string()])
                .unwrap();
            conn.execute("INSERT INTO book(content_hash,title,added_at,format) VALUES(?1,'Inventory Book',1,'epub')", [hash.as_str()]).unwrap();
            conn.execute(
                "INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(?1,(SELECT row_id FROM book WHERE content_hash=?2),'inventory.epub','fingerprint',1)",
                [dir_id.to_string().as_str(), hash.as_str()],
            )
            .unwrap();
            assert!(!db::state_inventory(&conn).unwrap().is_empty());
            conn.execute("DELETE FROM sync_outbox", []).unwrap();
        }

        manager.sync_state_under_gate(None).await.unwrap();

        let pushed = pushed_kinds.lock().unwrap();
        assert!(pushed.iter().any(|kind| kind == "directory_lifecycle"));
        assert!(pushed.iter().any(|kind| kind == "book_lifecycle"));
        assert!(pushed.iter().any(|kind| kind == "placement"));
        assert!(db::get_pending_mutations(&manager.database.get().unwrap()).unwrap().is_empty());
        server.abort();
    }

    /// Reading-position validity is a property of the wire type, not of a
    /// batch-level check. `validate_pull_batch` polices revision ordering.
    #[test]
    fn invalid_reading_positions_cannot_be_represented_on_the_wire() {
        assert!(library_model::ReadingPosition::parse("not-a-cfi").is_err());
        assert!(library_model::ReadingPosition::parse("epubcfi(/6/2)").is_ok());
        assert!(library_model::ReadingPosition::parse("pdfpage(3)").is_ok());
    }

    #[test]
    fn pull_batch_requires_strict_sequences_after_the_cursor() {
        let valid = pull_response(vec![pulled_change(11, deleted(1)), pulled_change(12, deleted(2))], LibraryRevision::new(12).ok(), false);
        assert_eq!(validate_pull_batch(&valid, cursor(LibraryRevision::new(10).ok())), Ok(()));

        let repeated = pull_response(vec![pulled_change(11, deleted(1)), pulled_change(11, deleted(2))], LibraryRevision::new(11).ok(), false);
        assert!(validate_pull_batch(&repeated, cursor(LibraryRevision::new(10).ok())).is_err());
        assert!(validate_pull_batch(&valid, cursor(LibraryRevision::new(11).ok())).is_err());
    }

    #[tokio::test]
    async fn production_client_drains_all_pull_pages_before_finishing() {
        #[derive(Clone)]
        struct PageState(Arc<std::sync::atomic::AtomicUsize>);

        async fn pull_page(axum::extract::State(state): axum::extract::State<PageState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            state.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let request = decode_sync_exchange(&headers, &body);
            let pull = match request.cursor.state_revision.map(LibraryRevision::get) {
                None => pull_response(vec![pulled_change(1, added(1, "Paged book", 10))], LibraryRevision::new(1).ok(), true),
                Some(1) => pull_response(vec![pulled_change(2, deleted(1))], LibraryRevision::new(2).ok(), false),
                _ => PullStateResponse { mutations: Vec::new(), next_cursor: request.cursor, has_more: false },
            };
            binary_sync_response(&SyncExchangeResponse { push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() }, pull })
        }

        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(pull_page)).with_state(PageState(calls.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        *manager.session.write().unwrap() = Some(sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());

        manager.pull_events_from_server(SyncCursor::default()).await.unwrap();

        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        let conn = manager.database.get().unwrap();
        assert_eq!(db::get_pull_cursor(&conn).unwrap(), cursor(LibraryRevision::new(2).ok()));
        server.abort();
    }

    #[tokio::test]
    async fn production_client_persists_exact_final_cursor_beyond_last_mutation() {
        async fn pull_page(headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            let request = decode_sync_exchange(&headers, &body);
            binary_sync_response(&SyncExchangeResponse {
                push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() },
                pull: pull_response(vec![pulled_change(1, added(1, "Cursor book", 10))], request.cursor.state_revision.or(LibraryRevision::new(10).ok()), false),
            })
        }
        let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(pull_page));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        *manager.session.write().unwrap() = Some(sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());

        manager.pull_events_from_server(SyncCursor::default()).await.unwrap();

        let conn = manager.database.get().unwrap();
        assert_eq!(db::get_pull_cursor(&conn).unwrap(), cursor(LibraryRevision::new(10).ok()));
        server.abort();
    }

    #[tokio::test]
    async fn production_client_persists_advanced_cursor_from_empty_final_page() {
        async fn pull_page(headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            let request = decode_sync_exchange(&headers, &body);
            assert_eq!(request.cursor.state_revision.map(LibraryRevision::get), Some(10));
            binary_sync_response(&SyncExchangeResponse { push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() }, pull: pull_response(Vec::new(), LibraryRevision::new(12).ok(), false) })
        }
        let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(pull_page));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        *manager.session.write().unwrap() = Some(sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        {
            let conn = manager.database.get().unwrap();
            db::set_sync_metadata(&conn, "last_pull_server_seq", "10").unwrap();
            assert_eq!(db::get_pull_cursor(&conn).unwrap(), cursor(LibraryRevision::new(10).ok()));
        }

        manager.pull_events_from_server(cursor(LibraryRevision::new(10).ok())).await.unwrap();

        let conn = manager.database.get().unwrap();
        assert_eq!(db::get_pull_cursor(&conn).unwrap(), cursor(LibraryRevision::new(12).ok()));
        assert_eq!(db::get_sync_metadata(&conn, "last_pull_server_seq").unwrap(), None);
        server.abort();
    }

    #[tokio::test]
    async fn production_client_uses_exchange_then_drains_remaining_pull_pages() {
        #[derive(Clone)]
        struct ExchangeState {
            exchanges: Arc<std::sync::atomic::AtomicUsize>,
            pulls: Arc<std::sync::atomic::AtomicUsize>,
        }

        // Every page travels over the exchange endpoint: the first carries the
        // push and the first pull page, and each later page is the same request
        // with an advanced cursor and nothing to push.
        async fn exchange_page(axum::extract::State(state): axum::extract::State<ExchangeState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            let request = decode_sync_exchange(&headers, &body);
            match request.cursor.state_revision.map(LibraryRevision::get) {
                None => {
                    state.exchanges.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    binary_sync_response(&SyncExchangeResponse {
                        push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() },
                        pull: pull_response(vec![pulled_change(1, added(1, "Exchanged book", 10))], LibraryRevision::new(1).ok(), true),
                    })
                }
                Some(1) => {
                    state.pulls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    assert!(request.mutations.is_empty(), "a drain page has nothing left to push");
                    binary_sync_response(&SyncExchangeResponse { push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() }, pull: pull_response(vec![pulled_change(2, deleted(1))], LibraryRevision::new(2).ok(), false) })
                }
                other => panic!("unexpected cursor {other:?}"),
            }
        }

        let state = ExchangeState { exchanges: Arc::new(std::sync::atomic::AtomicUsize::new(0)), pulls: Arc::new(std::sync::atomic::AtomicUsize::new(0)) };
        let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(exchange_page)).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        *manager.session.write().unwrap() = Some(sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());

        manager.exchange_events_with_server(SyncCursor::default(), None).await.unwrap();

        assert_eq!(state.exchanges.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(state.pulls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let conn = manager.database.get().unwrap();
        assert_eq!(db::get_pull_cursor(&conn).unwrap(), cursor(LibraryRevision::new(2).ok()));
        server.abort();
    }

    #[tokio::test]
    async fn push_and_pull_pagination_advance_in_the_same_exchange_loop() {
        #[derive(Clone)]
        struct ExchangeState(Arc<std::sync::atomic::AtomicUsize>);

        async fn exchange_page(axum::extract::State(state): axum::extract::State<ExchangeState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            let request = decode_sync_exchange(&headers, &body);
            let call = state.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let pull = match call {
                0 => {
                    assert_eq!(request.cursor, SyncCursor::default());
                    assert_eq!(request.mutations.len(), sync_common::MAX_PUSH_MUTATIONS);
                    pull_response(vec![pulled_change(1, added(1, "Concurrent pages", 10))], LibraryRevision::new(1).ok(), true)
                }
                1 => {
                    assert_eq!(request.cursor.state_revision, LibraryRevision::new(1).ok());
                    assert_eq!(request.mutations.len(), 1, "the next push page shares the next pull request");
                    pull_response(vec![pulled_change(2, deleted(1))], LibraryRevision::new(2).ok(), false)
                }
                _ => panic!("push and pull pagination should finish in two exchanges"),
            };
            binary_sync_response(&SyncExchangeResponse { push: PushMutationsResponse { accepted: request.mutations.iter().map(|mutation| mutation.mutation_id).collect(), rejected: Vec::new() }, pull })
        }

        let state = ExchangeState(Arc::new(std::sync::atomic::AtomicUsize::new(0)));
        let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(exchange_page)).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        *manager.session.write().unwrap() = Some(sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        {
            let mut conn = manager.database.get().unwrap();
            let tx = conn.transaction().unwrap();
            for sequence in 1..=sync_common::MAX_PUSH_MUTATIONS as u64 + 1 {
                let mutation = reading_change(sequence, sequence, "epubcfi(/6/2)", sequence);
                tx.execute(
                    "INSERT INTO sync_outbox(replica_seq,mutation_id,state_kind,state_key,body,changed_at) VALUES(?1,?2,'reading_position',?3,?4,?5)",
                    rusqlite::params![sequence as i64, mutation.mutation_id.to_string(), fixture_content_hash(sequence).as_str(), sync_common::wire::encode(&mutation.body).unwrap(), i64::try_from(mutation.changed_at).unwrap()],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }

        let result = manager.exchange_events_with_server(SyncCursor::default(), None).await.unwrap();

        assert!(result.mutation_issues.is_empty());
        assert_eq!(state.0.load(std::sync::atomic::Ordering::SeqCst), 2);
        let conn = manager.database.get().unwrap();
        assert!(db::get_pending_mutations(&conn).unwrap().is_empty());
        assert_eq!(db::get_pull_cursor(&conn).unwrap(), cursor(LibraryRevision::new(2).ok()));
        server.abort();
    }

    #[tokio::test]
    async fn transient_rejection_does_not_starve_later_push_pages() {
        #[derive(Clone)]
        struct ExchangeState {
            calls: Arc<std::sync::atomic::AtomicUsize>,
            rejected: Arc<std::sync::Mutex<Option<sync_common::MutationId>>>,
        }
        async fn exchange_page(axum::extract::State(state): axum::extract::State<ExchangeState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            let request = decode_sync_exchange(&headers, &body);
            let call = state.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let push = if call == 0 {
                assert_eq!(request.mutations.len(), sync_common::MAX_PUSH_MUTATIONS);
                let rejected = request.mutations[0].mutation_id;
                *state.rejected.lock().unwrap() = Some(rejected);
                PushMutationsResponse {
                    accepted: request.mutations[1..].iter().map(|mutation| mutation.mutation_id).collect(),
                    rejected: vec![MutationRejection { mutation_id: rejected, reason: MutationRejectionReason::MissingDependency }],
                }
            } else {
                assert_eq!(request.mutations.len(), 1);
                PushMutationsResponse { accepted: vec![request.mutations[0].mutation_id], rejected: Vec::new() }
            };
            binary_sync_response(&SyncExchangeResponse { push, pull: PullStateResponse { mutations: Vec::new(), next_cursor: request.cursor, has_more: false } })
        }

        let state = ExchangeState { calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)), rejected: Arc::new(std::sync::Mutex::new(None)) };
        let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(exchange_page)).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        *manager.session.write().unwrap() = Some(sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        {
            let mut conn = manager.database.get().unwrap();
            let tx = conn.transaction().unwrap();
            for sequence in 1..=sync_common::MAX_PUSH_MUTATIONS as u64 + 1 {
                let mutation = reading_change(sequence, sequence, "epubcfi(/6/2)", sequence);
                tx.execute(
                    "INSERT INTO sync_outbox(replica_seq,mutation_id,state_kind,state_key,body,changed_at) VALUES(?1,?2,'reading_position',?3,?4,?5)",
                    rusqlite::params![sequence as i64, mutation.mutation_id.to_string(), fixture_content_hash(sequence).as_str(), sync_common::wire::encode(&mutation.body).unwrap(), i64::try_from(mutation.changed_at).unwrap()],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }

        let exchange = manager.exchange_events_with_server(SyncCursor::default(), None).await.expect("a deferred page still completes the exchange");

        assert_eq!(state.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        let rejected = state.rejected.lock().unwrap().unwrap();
        assert_eq!(exchange.mutation_issues, vec![sync_common::MutationIssue::Retryable(MutationRejection { mutation_id: rejected, reason: MutationRejectionReason::MissingDependency })]);
        let conn = manager.database.get().unwrap();
        let pending = db::get_pending_mutations(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].mutation_id, rejected);
        server.abort();
    }

    fn queue_outbox_reading_change(manager: &SyncManager, mutation: &StateMutation, sequence: u64) {
        let conn = manager.database.get().unwrap();
        conn.execute(
            "INSERT INTO sync_outbox(replica_seq,mutation_id,state_kind,state_key,body,changed_at) VALUES(?1,?2,'reading_position',?3,?4,?5)",
            rusqlite::params![sequence as i64, mutation.mutation_id.to_string(), fixture_content_hash(sequence).as_str(), sync_common::wire::encode(&mutation.body).unwrap(), i64::try_from(mutation.changed_at).unwrap()],
        )
        .unwrap();
    }

    /// A deferred mutation means "ask again later", not "this exchange
    /// failed". Failing the whole synchronization strands everything the
    /// exchange did accomplish -- notably asset transfers and the completion
    /// checkpoint -- behind one mutation the server will not take yet, which
    /// for a device whose clock is ahead never resolves on its own.
    #[tokio::test]
    async fn a_deferred_mutation_does_not_fail_the_surrounding_synchronization() {
        #[derive(Clone)]
        struct ExchangeState {
            deferred: Arc<std::sync::Mutex<Option<sync_common::MutationId>>>,
        }
        async fn exchange_page(axum::extract::State(state): axum::extract::State<ExchangeState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            let request = decode_sync_exchange(&headers, &body);
            assert_eq!(request.mutations.len(), 2, "both queued positions travel in one page");
            let deferred = request.mutations[0].mutation_id;
            *state.deferred.lock().unwrap() = Some(deferred);
            binary_sync_response(&SyncExchangeResponse {
                push: PushMutationsResponse { accepted: vec![request.mutations[1].mutation_id], rejected: vec![MutationRejection { mutation_id: deferred, reason: MutationRejectionReason::TimestampTooFarFuture }] },
                pull: pull_response(vec![pulled_change(1, added(1, "Pulled while deferred", 10))], LibraryRevision::new(1).ok(), false),
            })
        }

        let state = ExchangeState { deferred: Arc::new(std::sync::Mutex::new(None)) };
        let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(exchange_page)).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        *manager.session.write().unwrap() = Some(sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        for sequence in 1..=2 {
            queue_outbox_reading_change(&manager, &reading_change(sequence, sequence, "epubcfi(/6/2)", sequence), sequence);
        }

        let outcome = manager.synchronize(SyncPolicy { assets: AssetSync::Transfer(TransferOrigin::Background), reading_positions: ReadingPositionSync::Immediate }).await;

        // Side effects first, so this test reports what the exchange actually
        // did before it reports how it answered.
        let conn = manager.database.get().unwrap();
        let pending = db::get_pending_mutations(&conn).unwrap();
        assert_eq!(pending.len(), 1, "the deferred mutation stays queued for a later attempt");
        assert_eq!(pending[0].mutation_id, state.deferred.lock().unwrap().unwrap());
        assert_eq!(db::get_pull_cursor(&conn).unwrap(), cursor(LibraryRevision::new(1).ok()), "the pulled page is still applied");

        let outcome = outcome.expect("one deferred mutation must not fail the synchronization");
        assert_eq!(outcome.mutation_issues, vec![sync_common::MutationIssue::Retryable(MutationRejection { mutation_id: state.deferred.lock().unwrap().unwrap(), reason: MutationRejectionReason::TimestampTooFarFuture })]);
        assert!(db::get_sync_metadata(&conn, "last_successful_sync_at").unwrap().is_some(), "the exchange completed, so the sync is checkpointed");
        server.abort();
    }

    /// When the server's state regresses beneath a device's cursor the device
    /// cannot make progress in either direction. Recovery is the existing
    /// republish path: drop the unusable cursor and re-reconcile, rather than
    /// failing every future synchronization.
    #[tokio::test]
    async fn a_rejected_cursor_is_dropped_and_the_exchange_recovers() {
        #[derive(Clone)]
        struct ExchangeState {
            calls: Arc<std::sync::atomic::AtomicUsize>,
        }
        async fn exchange_page(axum::extract::State(state): axum::extract::State<ExchangeState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            use axum::response::IntoResponse;

            let request = decode_sync_exchange(&headers, &body);
            state.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match request.cursor.state_revision.map(LibraryRevision::get) {
                // The server has regressed beneath this cursor and says so.
                Some(99) => axum::http::StatusCode::GONE.into_response(),
                None => binary_sync_response(&SyncExchangeResponse {
                    push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() },
                    pull: pull_response(vec![pulled_change(1, added(1, "Republished", 10))], LibraryRevision::new(1).ok(), false),
                }),
                Some(1) => binary_sync_response(&SyncExchangeResponse {
                    push: PushMutationsResponse { accepted: request.mutations.iter().map(|mutation| mutation.mutation_id).collect(), rejected: Vec::new() },
                    pull: pull_response(Vec::new(), LibraryRevision::new(1).ok(), false),
                }),
                other => panic!("unexpected cursor {other:?}"),
            }
        }

        let state = ExchangeState { calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)) };
        let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(exchange_page)).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        *manager.session.write().unwrap() = Some(sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        {
            let conn = manager.database.get().unwrap();
            db::set_pull_cursor(&conn, cursor(LibraryRevision::new(99).ok())).unwrap();
        }

        manager.sync_once().await.expect("a rejected cursor must be recoverable without operator intervention");

        assert_eq!(state.calls.load(std::sync::atomic::Ordering::SeqCst), 3, "recovery performs one clean pull before the one-shot publishing retry");
        let conn = manager.database.get().unwrap();
        assert_eq!(db::get_pull_cursor(&conn).unwrap(), cursor(LibraryRevision::new(1).ok()), "the device adopts the server's current head");
        assert_eq!(db::get_sync_metadata(&conn, CURSOR_RECOVERY_PENDING_KEY).unwrap(), None, "successful recovery clears its durable marker");
        server.abort();
    }

    #[test]
    fn beginning_cursor_recovery_atomically_persists_a_clean_cursor_and_resume_marker() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        {
            let conn = manager.database.get().unwrap();
            db::set_pull_cursor(&conn, cursor(LibraryRevision::new(99).ok())).unwrap();
        }

        manager.begin_cursor_recovery().unwrap();

        let conn = manager.database.get().unwrap();
        assert_eq!(db::get_pull_cursor(&conn).unwrap(), SyncCursor::default());
        assert_eq!(db::get_sync_metadata(&conn, CURSOR_RECOVERY_PENDING_KEY).unwrap().as_deref(), Some("1"));
    }

    /// A backup can retain a cell while losing its newest value. Missing-key
    /// inventory cannot detect that case, so cursor recovery must republish the
    /// local canonical state even when the restored server says every key is
    /// still present.
    #[tokio::test]
    async fn cursor_recovery_republishes_cells_that_still_exist_on_the_restored_server() {
        #[derive(Clone)]
        struct RecoveryState {
            calls: Arc<std::sync::atomic::AtomicUsize>,
            pushed_kinds: Arc<std::sync::Mutex<Vec<String>>>,
        }

        async fn inventory(body: axum::body::Bytes) -> axum::response::Response {
            let request: sync_common::StateInventoryRequest = sync_common::transport::decode(&body, sync_common::transport::MAX_DECODED_REQUEST_BYTES).unwrap();
            assert!(!request.cells.is_empty(), "recovery has local canonical cells to consider");
            binary_sync_response(&sync_common::StateInventoryResponse { missing: Vec::new() })
        }

        async fn exchange_page(axum::extract::State(state): axum::extract::State<RecoveryState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            use axum::response::IntoResponse;

            let request = decode_sync_exchange(&headers, &body);
            state.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if request.cursor.state_revision.map(LibraryRevision::get) == Some(99) {
                return axum::http::StatusCode::GONE.into_response();
            }
            state.pushed_kinds.lock().unwrap().extend(request.mutations.iter().map(|mutation| mutation.kind.clone()));
            binary_sync_response(&SyncExchangeResponse {
                push: PushMutationsResponse { accepted: request.mutations.iter().map(|mutation| mutation.mutation_id).collect(), rejected: Vec::new() },
                pull: pull_response(Vec::new(), LibraryRevision::new(1).ok(), false),
            })
        }

        let state = RecoveryState { calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)), pushed_kinds: Arc::new(std::sync::Mutex::new(Vec::new())) };
        let app = axum::Router::new().route("/api/sync/inventory", axum::routing::post(inventory)).route("/api/sync/exchange", axum::routing::post(exchange_page)).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        *manager.session.write().unwrap() = Some(sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        {
            let conn = manager.database.get().unwrap();
            let hash = fixture_content_hash(77);
            conn.execute("INSERT INTO book(content_hash,title,added_at,format) VALUES(?1,'Newer local value',10,'epub')", [hash.as_str()]).unwrap();
            assert!(!db::state_inventory(&conn).unwrap().is_empty());
            conn.execute("DELETE FROM sync_outbox", []).unwrap();
            db::set_pull_cursor(&conn, cursor(LibraryRevision::new(99).ok())).unwrap();
        }
        // Model a client that already performed its ordinary opening inventory
        // pass before the server restore became visible.
        manager.inventory_checked.store(true, Ordering::Release);

        manager.sync_once().await.expect("cursor recovery must republish local canonical state");

        assert_eq!(state.calls.load(std::sync::atomic::Ordering::SeqCst), 3, "one rejected exchange is followed by a clean pull and one publishing retry");
        assert!(state.pushed_kinds.lock().unwrap().iter().any(|kind| kind == sync_common::mutation_kind::BOOK_LIFECYCLE), "the newer local lifecycle value must be republished even though its key still exists remotely");
        assert_eq!(db::get_sync_metadata(&manager.database.get().unwrap(), CURSOR_RECOVERY_PENDING_KEY).unwrap(), None, "successful recovery clears its durable marker");
        server.abort();
    }

    fn reading_change(sequence: u64, content_hash: u64, cfi: &str, changed_at: u64) -> StateMutation {
        StateMutation {
            mutation_id: sync_common::MutationId::new(),
            body: MutationBody::ReadingPosition { content_hash: fixture_content_hash(content_hash), value: ReadingPositionState { location: library_model::ReadingPosition::parse(cfi).unwrap(), progress: 0.0 } },
            changed_at: millis(changed_at),
            replica_seq: local_seq(sequence),
        }
    }

    fn transmittable(changes: Vec<StateMutation>) -> sync_common::PushBatcher {
        let queued = prepare_push(changes).unwrap();
        assert!(queued.untransmittable().is_empty(), "this fixture holds nothing larger than a batch");
        queued
    }

    #[test]
    fn push_pages_are_bounded_by_count_and_preserve_sequence_order() {
        let mut changes = transmittable((1..=sync_common::MAX_PUSH_MUTATIONS as u64 + 7).map(|sequence| reading_change(sequence, sequence, "epubcfi(/6/2)", sequence)).collect::<Vec<_>>());

        let first = changes.next_batch();
        let second = changes.next_batch();

        assert_eq!(first.len(), sync_common::MAX_PUSH_MUTATIONS);
        assert_eq!(second.len(), 7);
        assert_eq!(first.first().unwrap().replica_seq.get(), 1);
        assert_eq!(second.first().unwrap().replica_seq.get(), sync_common::MAX_PUSH_MUTATIONS as u64 + 1);
    }

    /// The service refuses a request larger than its decoded request limit, and
    /// no batching makes an over-limit mutation fit. Left at the head of the
    /// outbox it fails every exchange, so each retry blocks every later
    /// mutation behind it. It must be set aside without being acknowledged:
    /// dropping it would leave this replica silently diverged from the server.
    #[test]
    fn a_mutation_too_large_to_transmit_does_not_block_the_rest_of_the_outbox() {
        let ahead = reading_change(1, 1, "epubcfi(/6/2)", 1);
        let oversized = annotation_change(2, "x".repeat(sync_common::MAX_PUSH_BATCH_BYTES + 1));
        let behind = reading_change(3, 3, "epubcfi(/6/4)", 3);

        let mut queued = prepare_push(vec![ahead.clone(), oversized.clone(), behind.clone()]).unwrap();

        assert_eq!(queued.untransmittable(), &[oversized.mutation_id], "only the untransmittable mutation is set aside");
        let page = queued.next_batch();
        assert_eq!(page.iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>(), vec![ahead.mutation_id, behind.mutation_id], "the mutations queued behind it still go out, in sequence order");
        assert!(queued.is_empty(), "the exchange loop has nothing left to spin on");
    }

    #[test]
    fn push_pages_stop_before_the_encoded_byte_target() {
        let large_text = "x".repeat(64 * 1024);
        let mut queued = transmittable((1..=100_u64).map(|sequence| annotation_change(sequence, large_text.clone())).collect::<Vec<_>>());

        let page = queued.next_batch();
        // The budget is spent in transmitted bytes, so the page is measured in
        // the same wire encoding the request carries.
        let encoded_bytes = page.iter().map(|mutation| sync_common::transport::encode(mutation).unwrap().len()).sum::<usize>();

        assert!(!page.is_empty());
        assert!(page.len() < 100);
        assert!(encoded_bytes <= sync_common::MAX_PUSH_BATCH_BYTES);
        assert!(!queued.is_empty());
    }

    fn annotation_change(sequence: u64, text: String) -> StateMutation {
        StateMutation {
            mutation_id: sync_common::MutationId::new(),
            body: MutationBody::Annotation {
                annotation_id: uuid::Uuid::new_v4().to_string(),
                value: AnnotationState {
                    content_hash: fixture_content_hash(sequence),
                    anchor: AnnotationAnchor::EpubCfi { cfi: "epubcfi(/6/2)".to_owned() },
                    exact_text: text.clone(),
                    style: AnnotationStyle::Highlight,
                    color: "yellow".to_owned(),
                    note: text,
                    created_at: millis(1),
                    modified_at: millis(1),
                    deleted: false,
                },
            },
            changed_at: millis(sequence),
            replica_seq: local_seq(sequence),
        }
    }

    #[test]
    fn reading_upload_waits_for_thirty_seconds_of_global_quiet() {
        let changes = vec![
            reading_change(1, 1, "epubcfi(/6/2)", 60_000),
            StateMutation { mutation_id: sync_common::MutationId::new(), body: added(3, "send-now", 1), changed_at: millis(1), replica_seq: local_seq(2) },
            reading_change(3, 2, "epubcfi(/6/4)", 95_000),
        ];
        let (ready, remaining) = mutations_ready_for_push(changes, Some(Duration::from_secs(30)), 100_000).unwrap();

        assert_eq!(ready.len(), 1);
        assert!(matches!(ready[0].body, MutationBody::BookLifecycle { .. }));
        assert_eq!(remaining, Some(Duration::from_secs(25)));
    }

    #[test]
    fn reading_upload_is_released_after_the_quiet_period() {
        let changes = vec![reading_change(1, 1, "epubcfi(/6/6)", 70_000)];
        let (ready, remaining) = mutations_ready_for_push(changes, Some(Duration::from_secs(30)), 100_000).unwrap();

        assert_eq!(ready.len(), 1);
        assert!(remaining.is_none());
    }

    #[test]
    fn pathological_future_reading_timestamp_is_rejected_instead_of_scheduling_an_unbounded_delay() {
        let now_millis = 100_000;
        let changes = vec![reading_change(1, 1, "epubcfi(/6/8)", now_millis + sync_common::MAX_LWW_FUTURE_SKEW_MS + 1)];
        let error = mutations_ready_for_push(changes, Some(Duration::from_secs(30)), now_millis).unwrap_err();

        assert!(error.to_string().contains("pathologically far in the future"));
    }

    #[test]
    fn push_coalesces_reading_positions_per_book_by_winning_version() {
        let changes = vec![
            reading_change(1, 1, "epubcfi(/6/10)", 10),
            StateMutation { mutation_id: sync_common::MutationId::new(), body: added(3, "preserved", 1), changed_at: millis(1), replica_seq: local_seq(2) },
            reading_change(3, 2, "epubcfi(/6/12)", 30),
            reading_change(4, 1, "epubcfi(/6/14)", 20),
            // Later local sequence, but an older wall clock: the server retains
            // sequence 3, so client compaction must make the same choice.
            reading_change(5, 2, "epubcfi(/6/16)", 20),
        ];

        let coalesced = coalesce_mutations_for_push(changes);
        assert_eq!(coalesced.iter().map(|change| change.replica_seq.get()).collect::<Vec<_>>(), vec![2, 3, 4]);
        assert!(matches!(&coalesced[1].body, MutationBody::ReadingPosition { value: ReadingPositionState { location, .. }, .. } if location.as_str() == "epubcfi(/6/12)"));
        assert!(matches!(&coalesced[2].body, MutationBody::ReadingPosition { value: ReadingPositionState { location, .. }, .. } if location.as_str() == "epubcfi(/6/14)"));
    }

    #[test]
    fn push_coalesces_every_protocol_singleton_but_preserves_append_events() {
        let dir_id = test_support::fixture_dir_id("singleton-dir");
        let changes = vec![
            StateMutation {
                mutation_id: sync_common::MutationId::new(),
                // The tombstone wins an exact wall-clock tie through conflict_rank,
                // even though the later local sequence below says present.
                body: MutationBody::DirectoryLifecycle { dir_id: dir_id.clone(), value: library_model::DirectoryLifecycleState::Deleted },
                changed_at: millis(10),
                replica_seq: local_seq(1),
            },
            StateMutation { mutation_id: sync_common::MutationId::new(), body: added(2, "canonical lifecycle", 10), changed_at: millis(10), replica_seq: local_seq(3) },
            StateMutation { mutation_id: sync_common::MutationId::new(), body: MutationBody::DirectoryLifecycle { dir_id, value: library_model::DirectoryLifecycleState::Present }, changed_at: millis(10), replica_seq: local_seq(4) },
            reading_change(6, 1, "epubcfi(/6/18)", 10),
            reading_change(7, 1, "epubcfi(/6/20)", 20),
        ];

        let coalesced = coalesce_mutations_for_push(changes);
        assert_eq!(coalesced.iter().map(|change| change.replica_seq.get()).collect::<Vec<_>>(), vec![1, 3, 7]);
        assert!(matches!(&coalesced[0].body, MutationBody::DirectoryLifecycle { value: library_model::DirectoryLifecycleState::Deleted, .. }));
        assert!(matches!(&coalesced[1].body, MutationBody::BookLifecycle { .. }));
    }

    #[test]
    fn coalesced_push_does_not_checkpoint_across_an_interleaved_rejection() {
        let changes = coalesce_mutations_for_push(vec![
            reading_change(1, 1, "epubcfi(/6/18)", 10),
            StateMutation { mutation_id: sync_common::MutationId::new(), body: added(2, "barrier", 1), changed_at: millis(1), replica_seq: local_seq(2) },
            reading_change(3, 1, "epubcfi(/6/22)", 20),
        ]);
        assert_eq!(changes.iter().map(|change| change.replica_seq).collect::<Vec<_>>(), vec![local_seq(2), local_seq(3)]);

        let response = PushMutationsResponse { accepted: vec![changes[1].mutation_id], rejected: vec![MutationRejection { mutation_id: changes[0].mutation_id, reason: MutationRejectionReason::TimestampTooFarFuture }] };
        let requested_ids = changes.iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>();
        let validated = validate_push_response(&requested_ids, &response).unwrap();
        assert_eq!(validated.accepted, vec![changes[1].mutation_id]);
        assert_eq!(validated.issues, vec![sync_common::MutationIssue::Retryable(MutationRejection { mutation_id: changes[0].mutation_id, reason: MutationRejectionReason::TimestampTooFarFuture })]);
    }

    #[test]
    fn push_acceptance_uses_mutation_identity() {
        let requested = vec![reading_change(1, 1, "epubcfi(/6/24)", 1), reading_change(2, 2, "epubcfi(/6/26)", 2)];
        let response = PushMutationsResponse { accepted: requested.iter().rev().map(|mutation| mutation.mutation_id).collect(), rejected: Vec::new() };
        let requested_ids = requested.iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>();
        assert_eq!(validate_push_response(&requested_ids, &response).unwrap().accepted, response.accepted);
    }

    #[test]
    fn permanent_rejection_is_reported_by_mutation_identity() {
        let requested = vec![reading_change(1, 1, "epubcfi(/6/24)", 1)];
        let response = PushMutationsResponse { accepted: Vec::new(), rejected: vec![MutationRejection { mutation_id: requested[0].mutation_id, reason: MutationRejectionReason::InvalidPayload }] };
        let requested_ids = requested.iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>();
        assert_eq!(validate_push_response(&requested_ids, &response).unwrap().issues, vec![sync_common::MutationIssue::Rejected(MutationRejection { mutation_id: requested[0].mutation_id, reason: MutationRejectionReason::InvalidPayload })]);
    }

    #[test]
    fn permanent_rejection_removes_poison_mutation_from_durable_outbox() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let requested = reading_change(1, 1, "epubcfi(/6/24)", 1);
        {
            let conn = manager.database.get().unwrap();
            conn.execute(
                "INSERT INTO sync_outbox(replica_seq,mutation_id,state_kind,state_key,body,changed_at) VALUES(?1,?2,'reading_position','1',?3,?4)",
                rusqlite::params![requested.replica_seq.get() as i64, requested.mutation_id.to_string(), sync_common::wire::encode(&requested.body).unwrap(), i64::try_from(requested.changed_at).unwrap()],
            )
            .unwrap();
        }
        let response = PushMutationsResponse { accepted: Vec::new(), rejected: vec![MutationRejection { mutation_id: requested.mutation_id, reason: MutationRejectionReason::InvalidPayload }] };

        assert!(manager.apply_push_response(&[requested.mutation_id], &response).is_err());

        let conn = manager.database.get().unwrap();
        assert!(db::get_pending_mutations(&conn).unwrap().is_empty());
    }

    #[tokio::test]
    async fn permanent_rejection_is_an_outcome_after_applying_the_exchange_pull_half() {
        async fn exchange(headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            let request = decode_sync_exchange(&headers, &body);
            let rejected = request.mutations.first().expect("the fixture publishes one mutation").mutation_id;
            binary_sync_response(&SyncExchangeResponse {
                push: PushMutationsResponse { accepted: Vec::new(), rejected: vec![MutationRejection { mutation_id: rejected, reason: MutationRejectionReason::InvalidPayload }] },
                pull: pull_response(vec![pulled_change(1, added(9, "Pulled despite rejection", 10))], LibraryRevision::new(1).ok(), false),
            })
        }

        let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(exchange));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        *manager.session.write().unwrap() = Some(sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        let requested = reading_change(1, 1, "epubcfi(/6/24)", 1);
        queue_outbox_reading_change(&manager, &requested, 1);

        let outcome = manager.exchange_events_with_server(SyncCursor::default(), None).await.unwrap();
        assert_eq!(outcome.mutation_issues, vec![sync_common::MutationIssue::Rejected(MutationRejection { mutation_id: requested.mutation_id, reason: MutationRejectionReason::InvalidPayload })]);
        assert!(outcome.assets_needed, "the pull half's asset signal must survive the rejection");

        let conn = manager.database.get().unwrap();
        assert!(db::get_pending_mutations(&conn).unwrap().is_empty());
        assert_eq!(db::get_pull_cursor(&conn).unwrap(), cursor(LibraryRevision::new(1).ok()));
        let pulled_title: String = conn.query_row("SELECT title FROM book WHERE content_hash=?1", [fixture_content_hash(9).as_str()], |row| row.get(0)).unwrap();
        assert_eq!(pulled_title, "Pulled despite rejection");
        server.abort();
    }

    #[test]
    fn incomplete_or_contradictory_acknowledgments_are_rejected() {
        let requested = vec![reading_change(1, 1, "epubcfi(/6/24)", 1), reading_change(2, 2, "epubcfi(/6/26)", 2)];
        let requested_ids = requested.iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>();
        let missing = PushMutationsResponse { accepted: vec![requested[0].mutation_id], rejected: Vec::new() };
        assert!(matches!(validate_push_response(&requested_ids, &missing), Err(PushResponseError::InvalidAcknowledgments(_))));

        let contradictory = PushMutationsResponse { accepted: vec![requested[0].mutation_id], rejected: vec![MutationRejection { mutation_id: requested[0].mutation_id, reason: MutationRejectionReason::TimestampTooFarFuture }] };
        assert!(matches!(validate_push_response(&requested_ids[..1], &contradictory), Err(PushResponseError::InvalidAcknowledgments(_))));
    }

    /// The whole path, through a real pull: a synchronized placement removal
    /// records its file work in the commit that advances the cursor, performs
    /// it, and retires it. Without the enqueue wired into the pull itself the
    /// replay machinery is dead code, so this covers the wiring the unit tests
    /// above cannot see.
    #[tokio::test]
    async fn a_pulled_placement_removal_records_performs_and_retires_its_file_work() {
        async fn pull_page(headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            let request = decode_sync_exchange(&headers, &body);
            let pull = match request.cursor.state_revision {
                None => pull_response(vec![pulled_change(1, MutationBody::Placement { dir_id: fixture_dir_id("Shelf"), content_hash: fixture_content_hash(1), present: false, origin_folder_id: None })], LibraryRevision::new(1).ok(), false),
                _ => PullStateResponse { mutations: Vec::new(), next_cursor: request.cursor, has_more: false },
            };
            binary_sync_response(&SyncExchangeResponse { push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() }, pull })
        }

        let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(pull_page));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        *manager.session.write().unwrap() = Some(sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());

        // A live placement whose file is on disk, which the pull then removes.
        // Seeded as remote-origin state: a local-origin insert would stamp the
        // cell with the current wall clock and the pulled removal would lose
        // the merge, which is a property of the reducer, not of this test.
        let dir_id = fixture_dir_id("Shelf");
        let content_hash = fixture_content_hash(1);
        {
            let conn = manager.database.get().unwrap();
            db::set_sync_metadata(&conn, "change_origin", "remote").unwrap();
            conn.execute("INSERT INTO dir(id, parent_id, name) VALUES (?1,'00000000-0000-0000-0000-000000000000','Shelf')", [dir_id.to_string()]).unwrap();
            conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [content_hash.as_str()]).unwrap();
            conn.execute(
                "INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES (?2,(SELECT row_id FROM book WHERE content_hash=?1),'book.epub','fp',1)",
                rusqlite::params![content_hash.as_str(), dir_id.to_string()],
            )
            .unwrap();
            db::set_sync_metadata(&conn, "change_origin", "local").unwrap();
        }
        let on_disk = temp.path().join("Shelf").join("book.epub");
        std::fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
        std::fs::write(&on_disk, b"book bytes").unwrap();

        manager.pull_events_from_server(SyncCursor::default()).await.unwrap();

        assert!(!on_disk.exists(), "the synchronized removal reaches the file, not only the database");
        let conn = manager.database.get().unwrap();
        assert_eq!(db::get_pull_cursor(&conn).unwrap().state_revision, LibraryRevision::new(1).ok());
        assert!(db::pending_fs_actions(&conn).unwrap().is_empty(), "work that completed is retired rather than replayed on every later sync");
        server.abort();
    }

    /// A library holding one book placed in one directory, with its file on
    /// disk and the placement already tombstoned — the committed state a pull
    /// page leaves behind when synchronization removes a placement.
    fn seed_tombstoned_placement(manager: &SyncManager, dir: &str, file_name: &str) -> (DirId, ContentHash, PathBuf) {
        let dir_id = fixture_dir_id(dir);
        let content_hash = fixture_content_hash(1);
        let conn = manager.database.get().unwrap();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES (?1,'00000000-0000-0000-0000-000000000000',?2)", rusqlite::params![dir_id.to_string(), dir]).unwrap();
        conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [content_hash.as_str()]).unwrap();
        conn.execute(
            "INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,deleted_at) VALUES (?2,(SELECT row_id FROM book WHERE content_hash=?1),?3,'fp',1,1)",
            rusqlite::params![content_hash.as_str(), dir_id.to_string(), file_name],
        )
        .unwrap();
        let on_disk = Path::new(&manager.library_root).join(dir).join(file_name);
        std::fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
        std::fs::write(&on_disk, b"book bytes").unwrap();
        (dir_id, content_hash, on_disk)
    }

    /// The pull that applies a removal advances the cursor in the same commit,
    /// so if the process dies before the file work runs, nothing will ever ask
    /// for it again: the file stays on disk, the next scan reads it as a new
    /// local placement, and the deletion is undone on every device. Recording
    /// the work in that same commit is what makes it survivable.
    #[test]
    fn filesystem_work_owed_by_a_committed_pull_survives_a_crash_before_it_runs() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let (dir_id, content_hash, on_disk) = seed_tombstoned_placement(&manager, "Shelf", "book.epub");

        // The pull page committed its cursor and its owed work, then the
        // process died before performing any of it.
        {
            let conn = manager.database.get().unwrap();
            let tx = conn.unchecked_transaction().unwrap();
            db::enqueue_fs_actions(&tx, &[db::FsAction::RetirePlacementFile { dir_id: dir_id.to_string(), content_hash: content_hash.to_string() }]).unwrap();
            tx.commit().unwrap();
        }
        assert!(on_disk.exists(), "the crash left the deletion unperformed");

        manager.run_owed_fs_actions();

        assert!(!on_disk.exists(), "the next run must still owe, and perform, the synchronized deletion");
        let conn = manager.database.get().unwrap();
        assert!(db::pending_fs_actions(&conn).unwrap().is_empty(), "completed work is retired so it is not repeated");
    }

    /// Replay is only safe if an action that already ran is harmless the second
    /// time — a crash can land after the effect but before the row is retired.
    #[test]
    fn replaying_completed_filesystem_work_is_harmless() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let (dir_id, content_hash, on_disk) = seed_tombstoned_placement(&manager, "Shelf", "book.epub");
        let action = db::FsAction::RetirePlacementFile { dir_id: dir_id.to_string(), content_hash: content_hash.to_string() };

        for _ in 0..2 {
            let conn = manager.database.get().unwrap();
            let tx = conn.unchecked_transaction().unwrap();
            db::enqueue_fs_actions(&tx, std::slice::from_ref(&action)).unwrap();
            tx.commit().unwrap();
            drop(conn);
            manager.run_owed_fs_actions();
        }

        assert!(!on_disk.exists());
        assert!(db::pending_fs_actions(&manager.database.get().unwrap()).unwrap().is_empty());
    }

    /// One unreadable row must not become a second wedge: everything queued
    /// behind it still owes the disk.
    #[test]
    fn an_unreadable_owed_action_is_discarded_without_blocking_the_rest() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let (dir_id, content_hash, on_disk) = seed_tombstoned_placement(&manager, "Shelf", "book.epub");

        {
            let conn = manager.database.get().unwrap();
            conn.execute("INSERT INTO pending_fs_action(action, created_at) VALUES(?1, 1)", [b"not a decodable action".to_vec()]).unwrap();
            let tx = conn.unchecked_transaction().unwrap();
            db::enqueue_fs_actions(&tx, &[db::FsAction::RetirePlacementFile { dir_id: dir_id.to_string(), content_hash: content_hash.to_string() }]).unwrap();
            tx.commit().unwrap();
        }

        manager.run_owed_fs_actions();

        assert!(!on_disk.exists(), "work queued behind an unreadable row still runs");
        assert!(db::pending_fs_actions(&manager.database.get().unwrap()).unwrap().is_empty());
    }

    /// Directory removals are recorded deepest first, so a parent is never
    /// asked to vanish while a child it still contains is queued behind it.
    #[test]
    fn owed_directory_removals_are_recorded_deepest_first() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let parent = Path::new(&manager.library_root).join("Shelf");
        let child = parent.join("Sub");
        std::fs::create_dir_all(&child).unwrap();

        {
            let conn = manager.database.get().unwrap();
            let tx = conn.unchecked_transaction().unwrap();
            let mut paths = vec![RelativeDirPath::parse("/Shelf").unwrap(), RelativeDirPath::parse("/Shelf/Sub").unwrap()];
            paths.sort_by_key(|path| std::cmp::Reverse(path.as_path().components().count()));
            db::enqueue_fs_actions(&tx, &paths.iter().map(|path| db::FsAction::RemoveDir { path: path.as_str().to_owned() }).collect::<Vec<_>>()).unwrap();
            tx.commit().unwrap();
        }

        manager.run_owed_fs_actions();

        assert!(!child.exists(), "the child directory is removed");
        assert!(!parent.exists(), "and the parent it emptied can then be removed too");
    }

    fn test_manager(root: &Path) -> SyncManager {
        test_manager_with_replica(root, "client")
    }

    fn test_manager_with_replica(root: &Path, replica_id: &str) -> SyncManager {
        let database = Database::open(root.join(crate::APP_HIDDEN_LIBRARY_DB_PATH), db::configure_connection).unwrap();
        {
            let conn = database.get().unwrap();
            db::init_db(&conn).unwrap();
        }
        SyncManager {
            database,
            replica_id: fixture_replica_id(replica_id),
            server_url: ServerUrl::parse("http://127.0.0.1:9").unwrap(),
            http_client: reqwest::Client::new(),
            library_id: fixture_library_id("library"),
            library_root: root.canonicalize().unwrap().to_string_lossy().into_owned(),
            event_tx: None,
            session: Arc::new(std::sync::RwLock::new(None)),
            transfer_runtime: TransferRuntime::default(),
            transfer_queue: Arc::new(TransferQueue::default()),
            pending_thumbnail_jobs: Arc::new(std::sync::Mutex::new(HashSet::new())),
            sync_gate: tokio::sync::Mutex::new(()),
            inventory_checked: AtomicBool::new(false),
        }
    }

    #[tokio::test]
    #[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
    async fn production_http_postgres_syncs_a_backend_outbox_to_a_second_replica() {
        let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL")).expect("valid E2E server URL");
        let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
        let session = sync_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in to E2E server");
        let library_id = fixture_library_id("backend-e2e");
        sync_client::upsert_library(&session, library_id.clone(), "Backend E2E".to_owned()).await.expect("create E2E library");

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = server_url;
        manager.library_id = library_id.clone();
        *manager.session.write().unwrap() = Some(session.clone());

        let conn = manager.database.get().unwrap();
        let transaction = conn.unchecked_transaction().unwrap();
        db::upsert_dir(&transaction, &fixture_dir_id("backend-e2e-dir"), &sync_common::ROOT_DIR_ID, &"Backend E2E".to_owned()).unwrap();
        transaction.commit().unwrap();

        manager.sync_once().await.expect("publish Backend outbox through real server");

        let observed = sync_client::exchange_changes(&reqwest::Client::new(), &session, library_id, fixture_replica_id("backend-e2e-observer"), Vec::new(), SyncCursor::default()).await.expect("pull through real server");
        assert!(observed.pull.mutations.iter().any(|change| change.mutation.kind == "directory_name"));
    }

    #[tokio::test]
    #[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
    async fn production_http_postgres_preserves_offline_outbox_and_reconciles_a_second_backend() {
        let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL")).expect("valid E2E server URL");
        let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
        let session = sync_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in to E2E server");
        let library_id = fixture_library_id("backend-e2e-offline");
        sync_client::upsert_library(&session, library_id.clone(), "Backend offline E2E".to_owned()).await.expect("create E2E library");

        let writer_root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(writer_root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut writer = test_manager_with_replica(writer_root.path(), "backend-e2e-offline-writer");
        writer.library_id = library_id.clone();
        let offline_session = sync_client::Session::try_new(ServerUrl::parse("http://127.0.0.1:9").unwrap(), session.user_id().to_owned(), session.email().to_owned(), session.token().to_owned()).unwrap();
        *writer.session.write().unwrap() = Some(offline_session);

        // The writer has no network route while it makes its local changes.
        // A failed exchange must leave the durable outbox intact for a later
        // reconnection, rather than losing either local register.
        let first_dir = fixture_dir_id("backend-e2e-offline-a");
        let second_dir = fixture_dir_id("backend-e2e-offline-b");
        {
            let conn = writer.database.get().unwrap();
            let transaction = conn.unchecked_transaction().unwrap();
            db::upsert_dir(&transaction, &first_dir, &sync_common::ROOT_DIR_ID, &"Offline A".to_owned()).unwrap();
            db::upsert_dir(&transaction, &second_dir, &sync_common::ROOT_DIR_ID, &"Offline B".to_owned()).unwrap();
            transaction.commit().unwrap();
        }
        assert!(writer.sync_once().await.is_err(), "the unavailable server must fail the sync attempt");
        assert!(writer.sync_once().await.is_err(), "a repeated offline attempt must not consume the durable outbox");
        {
            let conn = writer.database.get().unwrap();
            assert_eq!(db::get_pending_mutations(&conn).unwrap().len(), 6, "all three registers for each directory remain queued while offline");
        }

        writer.server_url = server_url.clone();
        *writer.session.write().unwrap() = Some(session.clone());
        writer.sync_once().await.expect("reconnect and publish offline outbox");
        {
            let conn = writer.database.get().unwrap();
            assert!(db::get_pending_mutations(&conn).unwrap().is_empty(), "server acknowledgement clears only the published outbox");
        }

        let observer_root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(observer_root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut observer = test_manager_with_replica(observer_root.path(), "backend-e2e-offline-observer");
        observer.server_url = server_url;
        observer.library_id = library_id;
        *observer.session.write().unwrap() = Some(session);
        observer.sync_once().await.expect("second backend pulls the writer's offline history");

        let conn = observer.database.get().unwrap();
        for (id, expected_name) in [(&first_dir, "Offline A"), (&second_dir, "Offline B")] {
            let actual_name: String = conn.query_row("SELECT name FROM dir WHERE id = ?1", [id.to_string()], |row| row.get(0)).unwrap();
            assert_eq!(actual_name, expected_name);
        }
    }

    #[tokio::test]
    #[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
    async fn production_http_postgres_drains_all_sync_pages_for_a_fresh_backend() {
        let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL")).unwrap();
        let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
        let session = sync_client::login(&server_url, "admin@bokheim.local", &password).await.unwrap();
        let library_id = fixture_library_id("backend-e2e-pagination");
        sync_client::upsert_library(&session, library_id.clone(), "Backend pagination E2E".to_owned()).await.unwrap();

        // Reading registers avoid filesystem materialization, leaving this as
        // a focused protocol test: 1,001 states must cross the 1,000-item
        // pull boundary and be drained by a fresh persistent client.
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let changes = (1..=sync_common::MAX_PULL_CHANGES as u64 + 1)
            .map(|index| StateMutation {
                mutation_id: sync_common::MutationId::new(),
                body: MutationBody::ReadingPosition { content_hash: fixture_content_hash(index), value: ReadingPositionState { location: library_model::ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 0.5 } },
                changed_at: now,
                replica_seq: ReplicaSeq::new(index).unwrap(),
            })
            .collect::<Vec<_>>();
        for page in changes.chunks(sync_common::MAX_PUSH_MUTATIONS) {
            let wire_page = page.iter().map(StateMutation::to_wire).collect::<Result<Vec<_>, _>>().unwrap();
            let response =
                sync_client::exchange_changes(&reqwest::Client::new(), &session, library_id.clone(), fixture_replica_id("backend-e2e-pagination-writer"), wire_page, SyncCursor::default()).await.expect("publish a page of reading registers");
            assert_eq!(response.push.accepted.len(), page.len());
            assert!(response.push.rejected.is_empty());
        }

        let observer_root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(observer_root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut observer = test_manager_with_replica(observer_root.path(), "backend-e2e-pagination-observer");
        observer.server_url = server_url;
        observer.library_id = library_id;
        *observer.session.write().unwrap() = Some(session);
        observer.sync_once().await.expect("drain every remote synchronization page");
        let conn = observer.database.get().unwrap();
        let positions: i64 = conn.query_row("SELECT COUNT(*) FROM book WHERE read_pos IS NOT NULL", [], |row| row.get(0)).unwrap();
        assert_eq!(positions, sync_common::MAX_PULL_CHANGES as i64 + 1);
        let cursor = db::get_pull_cursor(&conn).unwrap();
        assert!(cursor.reading_revision.is_some(), "the final reading cursor is persisted with the final page");
    }

    #[tokio::test]
    #[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
    async fn production_http_postgres_serves_asset_endpoints() {
        let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL")).expect("valid E2E server URL");
        let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
        let observer_session = sync_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in observer");
        let publisher_session = sync_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in publisher");
        assert_eq!(sync_client::current_account(&observer_session).await.unwrap().email, observer_session.email());

        let library_id = fixture_library_id("backend-e2e-assets");
        sync_client::upsert_library(&publisher_session, library_id.clone(), "Backend asset E2E".to_owned()).await.expect("create E2E library");
        assert!(sync_client::list_libraries(&observer_session).await.unwrap().iter().any(|library| library.library_id == library_id));

        let client = reqwest::Client::new();
        let book_bytes = test_epub(b"real server asset endpoint coverage");
        let book_hash = ContentHash::new(&blake3::hash(&book_bytes).to_hex().to_string());
        let manifest = sync_client::negotiate_blobs(&client, &publisher_session, &library_id, vec![BlobManifestEntry { content_hash: book_hash.clone(), size_bytes: book_bytes.len() as u64 }]).await.expect("negotiate missing book blob");
        assert_eq!(manifest.upload, vec![book_hash.clone()]);
        sync_client::upload_book_bytes(&client, &publisher_session, &library_id, &book_hash, book_bytes.clone()).await.expect("upload negotiated book blob");

        let book_url = publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/blobs/{}", book_hash.as_str()));
        let head = client.head(book_url.clone()).bearer_auth(publisher_session.token()).send().await.unwrap();
        assert_eq!(head.status(), reqwest::StatusCode::OK);
        assert_eq!(head.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &book_bytes.len().to_string());
        let range = client.get(book_url).bearer_auth(publisher_session.token()).header(reqwest::header::RANGE, "bytes=4-11").send().await.unwrap();
        assert_eq!(range.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert_eq!(range.bytes().await.unwrap().as_ref(), &book_bytes[4..12]);
        assert_eq!(sync_client::download_book_bytes(&client, &observer_session, &library_id, &book_hash).await.unwrap(), book_bytes);
        let owned = sync_client::negotiate_blobs(&client, &observer_session, &library_id, vec![BlobManifestEntry { content_hash: book_hash.clone(), size_bytes: book_bytes.len() as u64 }]).await.expect("negotiate uploaded book blob");
        assert_eq!(owned.owned, vec![book_hash.clone()]);

        let thumbnail = vec![0xff, 0xd8, 0xff, 0xd9];
        let thumbnail_url = publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/thumbnails/{}", book_hash.as_str()));
        assert_eq!(client.put(thumbnail_url.clone()).bearer_auth(publisher_session.token()).body(thumbnail.clone()).send().await.unwrap().status(), reqwest::StatusCode::OK);
        let thumbnail_head = client.head(thumbnail_url.clone()).bearer_auth(observer_session.token()).send().await.unwrap();
        assert_eq!(thumbnail_head.status(), reqwest::StatusCode::OK);
        assert_eq!(thumbnail_head.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &thumbnail.len().to_string());
        assert_eq!(sync_client::download_thumbnail_bytes(&client, &observer_session, &library_id, &book_hash).await.unwrap(), thumbnail);

        // Asset endpoints must reject malformed, unauthorized, and quota
        // exceeding requests without disturbing an already-owned blob.
        assert_eq!(client.head(publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/blobs/{}", book_hash.as_str()))).send().await.unwrap().status(), reqwest::StatusCode::UNAUTHORIZED);
        let invalid_range = client
            .get(publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/blobs/{}", book_hash.as_str())))
            .bearer_auth(publisher_session.token())
            .header(reqwest::header::RANGE, format!("bytes={}-", book_bytes.len()))
            .send()
            .await
            .unwrap();
        assert_eq!(invalid_range.status(), reqwest::StatusCode::RANGE_NOT_SATISFIABLE);
        let wrong_hash = ContentHash::new(&"f".repeat(64));
        let missing_length = client
            .put(publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/blobs/{}", wrong_hash.as_str())))
            .bearer_auth(publisher_session.token())
            .body(reqwest::Body::wrap_stream(futures_util::stream::iter(vec![Ok::<_, std::io::Error>(axum::body::Bytes::from(book_bytes.clone()))])))
            .send()
            .await
            .unwrap();
        assert_eq!(missing_length.status(), reqwest::StatusCode::LENGTH_REQUIRED);
        let wrong_hash_upload = client.put(publisher_session.server_url().endpoint(&format!("api/libraries/{library_id}/blobs/{}", wrong_hash.as_str()))).bearer_auth(publisher_session.token()).body(book_bytes.clone()).send().await.unwrap();
        assert_eq!(wrong_hash_upload.status(), reqwest::StatusCode::BAD_REQUEST);

        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL");
        let database = sqlx::PgPool::connect(&database_url).await.unwrap();
        sqlx::query("UPDATE user_storage_account SET quota_bytes = 0 WHERE user_id = $1").bind(publisher_session.user_id()).execute(&database).await.unwrap();
        let quota_hash = ContentHash::new(&"e".repeat(64));
        let quota = sync_client::negotiate_blobs(&client, &publisher_session, &library_id, vec![BlobManifestEntry { content_hash: quota_hash.clone(), size_bytes: 1 }]).await.expect("quota rejection is a valid manifest response");
        assert_eq!(quota.rejected.len(), 1);
        assert_eq!(quota.rejected[0].content_hash, quota_hash);
        assert_eq!(quota.rejected[0].reason, sync_common::api::assets::BlobManifestRejectionReason::QuotaExceeded);
    }

    #[tokio::test]
    async fn expiring_session_is_cleared_before_any_network_request() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let session = sync_client::Session::try_new_with_expiry(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string(), Some(now + 60)).unwrap();
        *manager.session.write().unwrap() = Some(session);

        manager.sync_once().await.unwrap();

        assert!(!manager.is_signed_in());
    }

    #[tokio::test]
    async fn unauthorized_sync_response_preserves_credentials_for_account_level_refresh() {
        let app = axum::Router::new().route("/api/sync/exchange", axum::routing::post(|| async { axum::http::StatusCode::UNAUTHORIZED }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        let session = sync_client::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap();
        *manager.session.write().unwrap() = Some(session);

        let error = manager.sync_once().await.unwrap_err();

        assert!(matches!(error, SyncError::AuthenticationRequired));
        assert!(manager.is_signed_in());
        server.abort();
    }

    #[test]
    fn materialize_remote_dir_move_renames_folder_and_prevents_scan_revert() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let dir_id = fixture_dir_id("d1");
        let dir_id_text = dir_id.to_string();
        {
            let conn = manager.database.get().unwrap();
            conn.execute("INSERT INTO dir(id, parent_id, name) VALUES (?1, '00000000-0000-0000-0000-000000000000', 'Old')", [dir_id_text.as_str()]).unwrap();
        }
        std::fs::create_dir_all(temp.path().join("Old")).unwrap();
        std::fs::write(temp.path().join("Old").join(".biblos_uuid"), &dir_id_text).unwrap();

        {
            let conn = manager.database.get().unwrap();
            conn.execute("UPDATE dir SET name='New' WHERE id=?1", [dir_id_text.as_str()]).unwrap();
        }
        manager.run_fs_action(&manager.database.get().unwrap(), &db::FsAction::MoveDir { dir_id: dir_id.to_string(), old_path: "Old".to_owned(), new_path: "New".to_owned() }).expect("a synchronized directory move relocates the folder");

        assert!(!temp.path().join("Old").exists());
        assert_eq!(std::fs::read_to_string(temp.path().join("New").join(".biblos_uuid")).unwrap(), dir_id_text);

        let conn = manager.database.get().unwrap();
        let before: i64 = conn.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0)).unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        db::upsert_dir(&tx, &dir_id, &sync_common::ROOT_DIR_ID, &"New".to_owned()).unwrap();
        tx.commit().unwrap();
        let after: i64 = conn.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0)).unwrap();
        assert_eq!(after, before, "scanner re-asserting materialized dir state must be a no-op");
    }

    #[test]
    fn materialize_remote_dir_deletes_parent_first_removes_bottom_up() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        std::fs::create_dir_all(temp.path().join("A").join("B")).unwrap();
        std::fs::write(temp.path().join("A").join(".biblos_uuid"), "a").unwrap();
        std::fs::write(temp.path().join("A").join("B").join(".biblos_uuid"), "b").unwrap();

        // Recorded deepest first, the order each applied pull page enqueues
        // them in.
        for path in ["A/B", "A"] {
            manager.remove_remote_dir(&RelativeDirPath::parse(path).unwrap());
        }

        assert!(!temp.path().join("A").exists());
    }

    #[test]
    fn materialize_remote_dir_deletes_nonempty_leaves_marker_intact() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let dir = temp.path().join("A");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".biblos_uuid"), "a").unwrap();
        std::fs::write(dir.join("stray.txt"), "user file").unwrap();

        manager.remove_remote_dir(&RelativeDirPath::parse("A").unwrap());

        assert!(dir.exists(), "non-empty dir should not be removed");
        assert!(dir.join(".biblos_uuid").exists(), "marker must survive so scanner keeps existing identity");
    }

    #[test]
    fn state_driven_download_job_uses_content_hash_identity() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let conn = manager.database.get().unwrap();
        let dir_id = fixture_dir_id("d1").to_string();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES (?1, '00000000-0000-0000-0000-000000000000', 'Shelf')", [dir_id.as_str()]).unwrap();
        conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [TEST_CONTENT_HASH]).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES (?2, (SELECT row_id FROM book WHERE content_hash=?1), 'book.epub', 'fp', 0)", rusqlite::params![TEST_CONTENT_HASH, dir_id])
            .unwrap();

        let job = manager.download_job(&conn, &test_content_hash(), TransferOrigin::UserInitiated).unwrap().expect("live placement should produce a download job");
        assert_eq!(job, TransferJob::download_blob(test_content_hash(), crate::BookPlacement::new(test_content_hash(), crate::RelativeBookPath::parse("/Shelf/aaaaaaaaaaaa.epub").unwrap()), TransferOrigin::UserInitiated,));
    }

    #[test]
    fn download_request_requires_and_resolves_a_live_placement() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        {
            let conn = manager.database.get().unwrap();
            conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [TEST_CONTENT_HASH]).unwrap();
        }

        assert!(manager.request_download(test_content_hash()).is_err());

        {
            let conn = manager.database.get().unwrap();
            conn.execute(
                "INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=?1), 'book.epub', 'fp', 0)",
                [TEST_CONTENT_HASH],
            )
            .unwrap();
        }
        manager.request_download(test_content_hash()).unwrap();

        let conn = manager.database.get().unwrap();
        assert_eq!(db::get_asset_requests(&conn, BlobKind::Book).unwrap(), vec![(test_content_hash(), TransferOrigin::UserInitiated)]);
    }

    #[test]
    fn durable_download_intent_rederives_work_after_queue_recreation() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        {
            let conn = manager.database.get().unwrap();
            conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [TEST_CONTENT_HASH]).unwrap();
            conn.execute(
                "INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=?1), 'book.epub', 'fp', 0)",
                [TEST_CONTENT_HASH],
            )
            .unwrap();
        }
        manager.request_download(test_content_hash()).unwrap();
        assert!(manager.transfer_queue.statuses().is_empty(), "the queue is not the durable source of intent");
        drop(manager);

        let restored = test_manager(temp.path());
        let conn = restored.database.get().unwrap();
        assert_eq!(db::get_asset_requests(&conn, BlobKind::Book).unwrap(), vec![(test_content_hash(), TransferOrigin::UserInitiated)]);
        assert!(restored.download_job(&conn, &test_content_hash(), TransferOrigin::UserInitiated).unwrap().is_some());
        assert!(restored.transfer_queue.statuses().is_empty());
    }
}
