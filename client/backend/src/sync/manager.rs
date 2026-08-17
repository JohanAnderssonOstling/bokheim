#![allow(clippy::too_many_arguments)]

use super::transfer::{TransferJob, TransferOrigin, TransferQueue, TransferRuntime, TransferWorker};
use crate::library::database as db;
use crate::platform::scanner::LibraryEvent;
use crate::RelativeDirPath;
use crate::{BlobKind, BookPlacement, ContentHash, RelativeBookPath, ServerUrl};
use crossbeam_channel::Sender;
use library_replica::{coalesce_mutations_for_push, MutationBody, StateMutation};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use sync_common::api::assets::{BlobManifestEntry, BlobManifestResponse, MAX_BLOB_MANIFEST_ENTRIES};
#[cfg(test)]
use sync_common::SyncExchangeResponse;
use sync_common::{DirId, LibraryId, PullStateResponse, PushMutationsResponse, ReplicaId};

use sync_client::{validate_pull_batch, validate_push_response, PushResponseError};

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
    pub deferred_mutations: Vec<sync_common::MutationRejection>,
}

const CURSOR_RECOVERY_PENDING_KEY: &str = "cursor_recovery_pending";

fn mutations_ready_for_push(mut mutations: Vec<StateMutation>, reading_debounce: Option<Duration>, now: std::time::SystemTime) -> Result<(Vec<StateMutation>, Option<Duration>), Box<dyn std::error::Error>> {
    let Some(reading_debounce) = reading_debounce else {
        return Ok((mutations, None));
    };
    let latest_reading_change = mutations.iter().filter(|mutation| matches!(mutation.body, MutationBody::ReadingPosition { .. })).map(|mutation| mutation.changed_at).max();
    let Some(latest_reading_change) = latest_reading_change else {
        return Ok((mutations, None));
    };
    let now_millis = u64::try_from(now.duration_since(std::time::UNIX_EPOCH)?.as_millis())?;
    if latest_reading_change > now_millis.saturating_add(sync_common::MAX_LWW_FUTURE_SKEW_MS) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "local reading-position timestamp is pathologically far in the future").into());
    }
    let debounce_millis = u64::try_from(reading_debounce.as_millis())?;
    let ready_at = latest_reading_change.saturating_add(debounce_millis);
    if ready_at <= now_millis {
        return Ok((mutations, None));
    }
    mutations.retain(|mutation| !matches!(mutation.body, MutationBody::ReadingPosition { .. }));
    Ok((mutations, Some(Duration::from_millis(ready_at - now_millis))))
}

/// A mutation paired with the wire form that will actually be transmitted.
/// Both the page budget and the service's own request limit are expressed in
/// transmitted bytes, so the size is measured there rather than on the local
/// representation, and the encoding is kept for the request itself.
struct PendingPush {
    mutation: StateMutation,
    wire: sync_common::WireMutation,
    encoded_bytes: usize,
}

impl PendingPush {
    fn encode(mutation: StateMutation) -> Result<Self, Box<dyn std::error::Error>> {
        let wire = mutation.to_wire()?;
        let encoded_bytes = sync_common::transport::encode(&wire)?.len();
        Ok(Self { mutation, wire, encoded_bytes })
    }

    fn exceeds_a_whole_page(&self) -> bool {
        self.encoded_bytes > sync_common::MAX_PUSH_MUTATION_BYTES
    }
}

/// Separates mutations that can be transmitted from those no page can carry.
///
/// A mutation larger than a whole page is refused by the service's request
/// limit however it is batched, and the client cannot page around it. Left at
/// the head of the queue it would fail every exchange and block every later
/// mutation behind it indefinitely. It is set aside so the rest continues to
/// synchronize, but deliberately left in the outbox: acknowledging it would
/// diverge this replica from the server with nothing left to notice.
fn partition_transmittable(changes: Vec<StateMutation>) -> Result<(VecDeque<PendingPush>, Vec<sync_common::MutationId>), Box<dyn std::error::Error>> {
    let mut transmittable = VecDeque::with_capacity(changes.len());
    let mut set_aside = Vec::new();
    for change in changes {
        let pending = PendingPush::encode(change)?;
        if pending.exceeds_a_whole_page() {
            set_aside.push(pending.mutation.mutation_id);
        } else {
            transmittable.push_back(pending);
        }
    }
    Ok((transmittable, set_aside))
}

fn take_push_page(mutations: &mut VecDeque<PendingPush>) -> Vec<PendingPush> {
    let mut page = Vec::with_capacity(sync_common::MAX_PUSH_MUTATIONS.min(mutations.len()));
    let mut encoded_bytes = 0_usize;
    while page.len() < sync_common::MAX_PUSH_MUTATIONS {
        let Some(next) = mutations.front() else { break };
        // Nothing in the queue exceeds a whole page, so the first mutation
        // always fits and the page can never come back empty.
        if !page.is_empty() && encoded_bytes.saturating_add(next.encoded_bytes) > sync_common::MAX_PUSH_MUTATION_BYTES {
            break;
        }
        encoded_bytes = encoded_bytes.saturating_add(next.encoded_bytes);
        page.push(mutations.pop_front().expect("the inspected mutation is still queued"));
    }
    page
}

pub type DbPool = Pool<SqliteConnectionManager>;

#[derive(Debug)]
pub struct SyncManager {
    db_pool: DbPool,
    replica_id: ReplicaId,
    server_url: ServerUrl,
    http_client: reqwest::Client,
    library_id: LibraryId,
    library_root: PathBuf,
    event_tx: Option<Sender<LibraryEvent>>,
    session: Arc<std::sync::RwLock<Option<crate::auth::Session>>>,
    session_path: PathBuf,
    transfer_runtime: TransferRuntime,
    transfer_queue: Arc<TransferQueue>,
    pending_thumbnail_jobs: Arc<std::sync::Mutex<HashSet<ContentHash>>>,
    sync_gate: tokio::sync::Mutex<()>,
    inventory_checked: AtomicBool,
}

impl SyncManager {
    pub(crate) fn new(
        db_pool: DbPool, replica_id: ReplicaId, server_url: ServerUrl, library_id: LibraryId, library_root: PathBuf, session_path: PathBuf, event_tx: Option<Sender<LibraryEvent>>, transfer_runtime: TransferRuntime,
        pending_thumbnail_jobs: Arc<std::sync::Mutex<HashSet<ContentHash>>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let http_client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;
        let transfer_queue = Arc::new(TransferQueue::with_history_db(db_pool.clone()));
        Ok(Self {
            db_pool,
            replica_id,
            server_url,
            http_client,
            library_id,
            library_root,
            event_tx,
            session: Arc::new(std::sync::RwLock::new(None)),
            session_path,
            transfer_runtime,
            transfer_queue,
            pending_thumbnail_jobs,
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

    pub fn current_session(&self) -> Option<crate::auth::Session> {
        self.session.read().ok().and_then(|s| s.clone())
    }

    pub fn server_url(&self) -> &ServerUrl {
        &self.server_url
    }

    pub fn bound_user_id(&self) -> Result<Option<String>, String> {
        let conn = self.db_pool.get().map_err(|error| error.to_string())?;
        db::get_sync_metadata(&conn, "bound_user_id").map_err(|error| error.to_string())
    }

    /// Binds this local library to the session's server-issued user. Switching
    /// accounts clears account-specific remote presence and pull-cursor state,
    /// while retaining the local library itself.
    pub fn adopt_session(&self, session: crate::auth::Session) -> Result<(), String> {
        if session.server_url() != &self.server_url {
            return Err(format!("session belongs to {}, but this library syncs with {}", session.server_url(), self.server_url));
        }
        let mut rebound = false;
        {
            let conn = self.db_pool.get().map_err(|e| e.to_string())?;
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
        crate::auth::save_session_to(&session, &self.session_path).map_err(|error| error.to_string())?;
        let mut guard = self.session.write().map_err(|_| "session lock is poisoned".to_string())?;
        *guard = Some(session);
        Ok(())
    }

    pub fn clear_session(&self) -> Result<(), String> {
        {
            let mut guard = self.session.write().map_err(|_| "session lock is poisoned".to_string())?;
            *guard = None;
        }
        crate::auth::clear_session_from(&self.session_path).map_err(|error| error.to_string())
    }

    pub(crate) async fn synchronize(&self, policy: SyncPolicy) -> Result<SyncOutcome, Box<dyn std::error::Error>> {
        let _sync_guard = self.sync_gate.lock().await;
        // Anything a previous run recorded but did not finish is owed to the
        // disk before this one advances the cursor any further; the pull that
        // queued it has already moved past it.
        self.run_owed_fs_actions();
        let reading_debounce = match policy.reading_positions {
            #[cfg(test)]
            ReadingPositionSync::Immediate => None,
            ReadingPositionSync::AfterQuietPeriod(duration) => Some(duration),
        };
        let (signed_in, reading_delay, assets_needed, deferred_mutations) = self.sync_state_under_gate(reading_debounce).await?;
        if !signed_in {
            return Ok(SyncOutcome::default());
        }
        if let AssetSync::Transfer(origin) = policy.assets {
            self.sync_assets(origin).await?;
        } else if assets_needed {
            self.sync_assets(TransferOrigin::Background).await?;
        }
        let completed_at = u64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis())?;
        let conn = self.db_pool.get()?;
        db::set_sync_metadata(&conn, "last_successful_sync_at", &completed_at.to_string())?;
        Ok(SyncOutcome { next_reading_upload_in: reading_delay, deferred_mutations })
    }

    #[cfg(test)]
    async fn sync_once(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.synchronize(SyncPolicy { assets: AssetSync::Transfer(TransferOrigin::Background), reading_positions: ReadingPositionSync::Immediate }).await.map(|_| ())
    }

    async fn sync_state_under_gate(&self, reading_debounce: Option<Duration>) -> Result<(bool, Option<Duration>, bool, Vec<sync_common::MutationRejection>), Box<dyn std::error::Error>> {
        if !self.is_signed_in() {
            // Local-only mode: nothing to push, nothing to pull.
            return Ok((false, None, false, Vec::new()));
        }
        if self.current_session().is_some_and(|session| if session.refresh_token().is_some() { session.refresh_expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) } else { session.expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) }) {
            self.clear_session().map_err(std::io::Error::other)?;
            log::warn!("Authentication credentials expired or are too close to expiry; sign in again before synchronizing");
            return Ok((false, None, false, Vec::new()));
        }

        let mut recovery_assets_needed = false;
        if self.cursor_recovery_pending()? {
            recovery_assets_needed |= self.resume_cursor_recovery().await?;
        }

        self.reconcile_state_inventory_once().await?;

        for attempt in 0..2 {
            let pull_cursor = {
                let conn = self.db_pool.get()?;
                db::get_pull_cursor(&conn)?
            };

            // One authenticated exchange publishes the local outbox and returns
            // the first incremental page from the resulting committed snapshot.
            let recover_cursor = {
                match self.exchange_events_with_server(pull_cursor, reading_debounce).await {
                    Ok((reading_delay, assets_needed, deferred)) => return Ok((true, reading_delay, recovery_assets_needed | assets_needed, deferred)),
                    Err(error) if attempt == 0 && Self::sync_error_is_cursor_rejected(error.as_ref()) => true,
                    Err(error) => return Err(error),
                }
            };
            // Keep the non-Send boxed exchange error in the scope above, so it
            // is destroyed before cursor recovery crosses another await point.
            if recover_cursor {
                self.begin_cursor_recovery()?;
                recovery_assets_needed |= self.resume_cursor_recovery().await?;
            }
        }
        unreachable!("cursor recovery retry must return")
    }

    fn sync_error_is_cursor_rejected(error: &(dyn std::error::Error + 'static)) -> bool {
        error.downcast_ref::<sync_client::AuthError>().is_some_and(sync_client::AuthError::is_cursor_rejected)
    }

    fn cursor_recovery_pending(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        Ok(db::get_sync_metadata(&conn, CURSOR_RECOVERY_PENDING_KEY)?.as_deref() == Some("1"))
    }

    fn begin_cursor_recovery(&self) -> Result<(), Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        let tx = conn.unchecked_transaction()?;
        db::set_pull_cursor(&tx, sync_common::SyncCursor::default())?;
        db::set_sync_metadata(&tx, CURSOR_RECOVERY_PENDING_KEY, "1")?;
        tx.commit()?;
        self.inventory_checked.store(false, Ordering::Release);
        Ok(())
    }

    async fn resume_cursor_recovery(&self) -> Result<bool, Box<dyn std::error::Error>> {
        // Pull first so local LWW reduction sees everything retained by the
        // restored server before canonical local state is republished.
        let (_, assets_needed) = self.pull_events_from_server_with_first(sync_common::SyncCursor::default(), None).await?;
        let cells = {
            let conn = self.db_pool.get()?;
            db::state_inventory(&conn)?
        };
        let inserted = {
            let mut conn = self.db_pool.get()?;
            db::enqueue_missing_state_cells(&mut conn, &cells)?
        };
        let conn = self.db_pool.get()?;
        conn.execute("DELETE FROM sync_metadata WHERE key=?1", [CURSOR_RECOVERY_PENDING_KEY])?;
        self.inventory_checked.store(true, Ordering::Release);
        log::warn!("Recovered a rejected synchronization cursor and queued {inserted} canonical cells for republishing");
        Ok(assets_needed)
    }

    async fn reconcile_state_inventory_once(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.inventory_checked.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let result = self.reconcile_state_inventory().await;
        if result.is_err() {
            self.inventory_checked.store(false, Ordering::Release);
        }
        result
    }

    async fn reconcile_state_inventory(&self) -> Result<(), Box<dyn std::error::Error>> {
        let cells = {
            let conn = self.db_pool.get()?;
            db::state_inventory(&conn)?
        };
        if cells.is_empty() {
            return Ok(());
        }
        let session = self.current_session().ok_or("not signed in")?;
        let mut missing = Vec::new();
        for chunk in cells.chunks(sync_common::MAX_STATE_INVENTORY_CELLS) {
            let requested = chunk.iter().cloned().collect::<HashSet<_>>();
            let response = match sync_client::compare_state_inventory(&self.http_client, &session, self.library_id.clone(), chunk.to_vec()).await {
                Ok(response) => response,
                Err(error) if error.is_not_found() => {
                    log::debug!("Server does not support state inventory reconciliation yet");
                    return Ok(());
                }
                Err(error) => return Err(Box::new(error)),
            };
            if response.missing.len() > chunk.len() || response.missing.iter().any(|cell| !requested.contains(cell)) {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "server returned an invalid state inventory response").into());
            }
            missing.extend(response.missing);
        }
        if missing.is_empty() {
            return Ok(());
        }
        let inserted = {
            let mut conn = self.db_pool.get()?;
            db::enqueue_missing_state_cells(&mut conn, &missing)?
        };
        log::info!("State inventory reconciliation queued {inserted} missing cells for library {}", self.library_id);
        Ok(())
    }

    pub async fn resume_transfers(&self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.is_signed_in() {
            return Ok(());
        }
        TransferWorker::new(
            self.http_client.clone(),
            self.server_url.clone(),
            self.library_id.clone(),
            self.library_root.clone(),
            self.db_pool.clone(),
            self.event_tx.clone(),
            Arc::clone(&self.session),
            self.session_path.parent().unwrap_or(Path::new(".")).join("remote-book-cache"),
            self.transfer_runtime.clone(),
            Arc::clone(&self.transfer_queue),
        )
        .run_pending(4)
        .await
        .map_err(|error| error as Box<dyn std::error::Error>)
    }

    fn handle_client_result<T>(&self, result: Result<T, sync_client::AuthError>) -> Result<T, Box<dyn std::error::Error>> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => Err(Box::new(error)),
        }
    }

    async fn sync_assets(&self, upload_origin: TransferOrigin) -> Result<(), Box<dyn std::error::Error>> {
        cleanup_stale_transfer_temps(self.library_root.as_path(), std::time::SystemTime::now() - Duration::from_secs(24 * 60 * 60));
        let conn = self.db_pool.get()?;
        let assets = db::get_book_assets(&conn)?;
        let mut jobs = Vec::new();
        let mut known_hashes = HashSet::new();

        // Record every existing local instance. Synced folder relationships do
        // not materialize additional copies of content already present locally.
        for asset in &assets {
            let local_path = self.resolve_book_path(&asset.rel_path);
            let thumb_path = self.thumbnail_path_for_content_hash(&asset.content_hash);
            db::upsert_blob_local_ref(&conn, BlobKind::Book, &asset.content_hash, &local_path, Path::new(&local_path).exists())?;
            db::upsert_blob_local_ref(&conn, BlobKind::Thumbnail, &asset.content_hash, &thumb_path, Path::new(&thumb_path).exists())?;
            known_hashes.insert(asset.content_hash.clone());
        }

        let upload_blob_hashes = db::get_locally_present_blob_hashes(&conn, BlobKind::Book)?;
        let remote_book_hashes = db::get_remote_asset_hashes(&conn, BlobKind::Book)?;
        let mut upload_blobs = Vec::with_capacity(upload_blob_hashes.len());
        for content_hash in upload_blob_hashes {
            if remote_book_hashes.contains(&content_hash) {
                continue;
            }
            let Some(local_path) = db::get_blob_local_path(&conn, BlobKind::Book, &content_hash)? else { continue };
            match std::fs::metadata(&local_path) {
                Ok(metadata) if metadata.is_file() => {
                    upload_blobs.push((content_hash, local_path, metadata.len()));
                }
                _ => db::upsert_blob_local_ref(&conn, BlobKind::Book, &content_hash, &local_path, false)?,
            }
        }
        let upload_thumb_hashes = db::get_locally_present_blob_hashes(&conn, BlobKind::Thumbnail)?;
        let remote_thumbnail_hashes = db::get_remote_asset_hashes(&conn, BlobKind::Thumbnail)?;
        let pending_thumbnail_jobs = self.pending_thumbnail_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let pending_requests = db::get_asset_requests(&conn, BlobKind::Book)?;
        let auto_download_candidates: HashSet<_> = if self.transfer_runtime.auto_download_limit_bytes().is_some() && self.transfer_runtime.allows_background_transfers() {
            assets.iter().filter(|asset| !Path::new(&self.resolve_book_path(&asset.rel_path)).exists()).map(|asset| asset.content_hash.clone()).collect()
        } else {
            HashSet::new()
        };
        drop(conn);

        let manifest = upload_blobs.iter().map(|(content_hash, _, size_bytes)| BlobManifestEntry { content_hash: content_hash.clone(), size_bytes: *size_bytes }).collect();
        let negotiated = self.negotiate_upload_blobs(manifest).await?;
        let remotely_present: HashSet<_> = negotiated.owned.into_iter().chain(negotiated.claimed).collect();
        let should_upload: HashSet<_> = negotiated.upload.into_iter().collect();
        for rejected in negotiated.rejected {
            log::warn!("Server rejected blob {} during batch admission: {:?}", rejected.content_hash, rejected.reason);
        }
        for (content_hash, local_path, _) in upload_blobs {
            if remotely_present.contains(&content_hash) {
                let conn = self.db_pool.get()?;
                db::record_remote_asset(&conn, BlobKind::Book, &content_hash)?;
                continue;
            }
            if !should_upload.contains(&content_hash) {
                continue;
            }
            jobs.push(TransferJob::UploadBlob { content_hash, local_path, placement: None, origin: upload_origin });
        }
        for content_hash in upload_thumb_hashes {
            if remote_thumbnail_hashes.contains(&content_hash) {
                continue;
            }
            let conn = self.db_pool.get()?;
            let Some(thumbnail_path) = db::get_blob_local_path(&conn, BlobKind::Thumbnail, &content_hash)? else {
                continue;
            };
            jobs.push(TransferJob::UploadThumbnail { thumbnail_hash: content_hash, thumbnail_path, book_content_hash: None, origin: upload_origin });
        }

        // A requested download is itself the authoritative presence check: a
        // successful GET records present and a 404 records missing.
        for (content_hash, origin) in pending_requests {
            let conn = self.db_pool.get()?;
            if db::is_blob_local(&conn, BlobKind::Book, &content_hash)? {
                db::complete_asset_request(&conn, BlobKind::Book, &content_hash)?;
                continue;
            }
            match self.download_job(&conn, &content_hash, origin) {
                Ok(Some(job)) => jobs.push(job),
                Ok(None) => {}
                Err(error) => log::warn!("Skipping invalid requested book download {content_hash}: {error}"),
            }
        }

        if let Some(limit) = self.transfer_runtime.auto_download_limit_bytes() {
            for content_hash in auto_download_candidates {
                let conn = self.db_pool.get()?;
                if db::is_blob_local(&conn, BlobKind::Book, &content_hash)? {
                    continue;
                }
                drop(conn);
                let Some(size) = self.remote_blob_size(&content_hash).await? else {
                    let conn = self.db_pool.get()?;
                    db::forget_remote_asset(&conn, BlobKind::Book, &content_hash)?;
                    continue;
                };
                let conn = self.db_pool.get()?;
                db::record_remote_asset(&conn, BlobKind::Book, &content_hash)?;
                if size > limit {
                    continue;
                }
                match self.download_job(&conn, &content_hash, TransferOrigin::Background) {
                    Ok(Some(job)) => jobs.push(job),
                    Ok(None) => {}
                    Err(error) => log::warn!("Skipping invalid automatic book download {content_hash}: {error}"),
                }
            }
        }

        // Thumbnail GETs likewise record either present or missing themselves.
        for content_hash in known_hashes {
            let conn = self.db_pool.get()?;
            let thumbnail_is_local = db::is_blob_local(&conn, BlobKind::Thumbnail, &content_hash)?;
            let book_is_local = db::is_blob_local(&conn, BlobKind::Book, &content_hash)?;
            let thumbnail_is_known_remote = remote_thumbnail_hashes.contains(&content_hash);
            let thumbnail_generation_pending = pending_thumbnail_jobs.contains(&content_hash);
            if !should_download_thumbnail(book_is_local, thumbnail_is_local, thumbnail_is_known_remote, thumbnail_generation_pending) {
                continue;
            }
            let Some(book_path) = db::get_book_paths_by_content_hash(&conn, &content_hash)?.into_iter().next() else {
                continue;
            };
            jobs.push(TransferJob::DownloadThumbnail { thumbnail_hash: content_hash, book_content_hash: book_path.content_hash, origin: upload_origin });
        }

        TransferWorker::new(
            self.http_client.clone(),
            self.server_url.clone(),
            self.library_id.clone(),
            self.library_root.clone(),
            self.db_pool.clone(),
            self.event_tx.clone(),
            Arc::clone(&self.session),
            self.session_path.parent().unwrap_or(Path::new(".")).join("remote-book-cache"),
            self.transfer_runtime.clone(),
            Arc::clone(&self.transfer_queue),
        )
        // A single queue consumer makes the priority ordering a strict phase:
        // synchronized database state is already committed above, thumbnails
        // finish before any full publication transfer begins.
        .run(jobs, 1)
        .await?;
        Ok(())
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

    async fn remote_blob_size(&self, content_hash: &ContentHash) -> Result<Option<u64>, Box<dyn std::error::Error>> {
        let token = self.current_session().ok_or("authentication is required")?.token().to_owned();
        let endpoint = self.server_url.endpoint(&format!("api/libraries/{}/blobs/{}", self.library_id, urlencoding::encode(content_hash.as_str())));
        let response = self.http_client.head(endpoint).bearer_auth(token).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = response.error_for_status()?;
        Ok(response.content_length())
    }

    pub fn request_downloads(&self, content_hashes: &[ContentHash]) -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = self.db_pool.get()?;
        let transaction = conn.transaction()?;
        for content_hash in content_hashes {
            let hashes = db::get_book_hashes(&transaction, content_hash.clone())?;
            if hashes.is_empty() {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("book {content_hash} has no live placement")).into());
            }
            for hash in hashes {
                db::add_asset_request(&transaction, BlobKind::Book, &hash, TransferOrigin::UserInitiated)?;
            }
        }
        transaction.commit()?;
        drop(conn);
        if let Some(tx) = &self.event_tx {
            for content_hash in content_hashes {
                let _ = tx.send(LibraryEvent::DownloadStatusChanged { content_hash: content_hash.clone(), state: crate::DownloadState::Queued });
            }
        }
        Ok(())
    }

    fn resolve_book_path(&self, rel_path: &RelativeBookPath) -> String {
        rel_path.resolve_under(&self.library_root).to_string_lossy().into_owned()
    }

    fn thumbnail_path_for_content_hash(&self, content_hash: &ContentHash) -> String {
        crate::sync::transfer::thumbnail_path_from_content_hash(&self.library_root, content_hash).to_string_lossy().into_owned()
    }

    fn download_job(&self, conn: &rusqlite::Connection, content_hash: &ContentHash, origin: TransferOrigin) -> Result<Option<TransferJob>, rusqlite::Error> {
        let book_paths = db::get_book_paths_by_content_hash(conn, content_hash)?;
        let Some(chosen) = book_paths.iter().find(|path| !Path::new(&self.resolve_book_path(&path.rel_path)).exists()).or_else(|| book_paths.first()) else {
            return Ok(None);
        };
        let materialized_path = db::materialize_download_path(conn, &self.library_root, &BookPlacement::new(chosen.content_hash.clone(), chosen.rel_path.clone()))?;
        let local_path = self.resolve_book_path(&materialized_path);
        db::upsert_blob_local_ref(conn, BlobKind::Book, content_hash, &local_path, Path::new(&local_path).exists())?;
        Ok(Some(TransferJob::DownloadBlob { content_hash: content_hash.clone(), placement: BookPlacement::new(chosen.content_hash.clone(), materialized_path), origin }))
    }

    async fn negotiate_upload_blobs(&self, blobs: Vec<BlobManifestEntry>) -> Result<BlobManifestResponse, Box<dyn std::error::Error>> {
        if blobs.is_empty() {
            return Ok(BlobManifestResponse::default());
        }
        let session = self.current_session().ok_or("not signed in")?;
        let mut combined = BlobManifestResponse::default();
        for chunk in blobs.chunks(MAX_BLOB_MANIFEST_ENTRIES) {
            let mut response = self.handle_client_result(sync_client::negotiate_blobs(&self.http_client, &session, &self.library_id, chunk.to_vec()).await)?;
            combined.owned.append(&mut response.owned);
            combined.claimed.append(&mut response.claimed);
            combined.upload.append(&mut response.upload);
            combined.rejected.append(&mut response.rejected);
        }
        Ok(combined)
    }

    fn apply_push_response(&self, requested_ids: &[sync_common::MutationId], push_response: &PushMutationsResponse) -> Result<Vec<sync_common::MutationRejection>, Box<dyn std::error::Error>> {
        let validated = validate_push_response(requested_ids, push_response)?;

        for rejection in &push_response.rejected {
            if rejection.reason.is_transient() {
                log::warn!("Server deferred local sync mutation {}: {:?}", rejection.mutation_id, rejection.reason);
            } else {
                log::error!("Server rejected local sync mutation {}: {:?}", rejection.mutation_id, rejection.reason);
            }
        }

        let completed = validated.accepted.iter().copied().chain(validated.permanent_rejections.iter().map(|(mutation_id, _)| *mutation_id)).collect::<Vec<_>>();
        if !completed.is_empty() {
            let conn = self.db_pool.get()?;
            db::acknowledge_mutations(&conn, &completed)?;
        }
        if let Some(&(mutation_id, reason)) = validated.permanent_rejections.first() {
            return Err(PushResponseError::PermanentRejection { mutation_id, reason }.into());
        }
        Ok(validated.transient_rejections.into_iter().map(|(mutation_id, reason)| sync_common::MutationRejection { mutation_id, reason }).collect())
    }

    async fn exchange_events_with_server(&self, starting_cursor: sync_common::SyncCursor, reading_debounce: Option<Duration>) -> Result<(Option<Duration>, bool, Vec<sync_common::MutationRejection>), Box<dyn std::error::Error>> {
        let (changes, reading_delay) = {
            let conn = self.db_pool.get()?;
            mutations_ready_for_push(coalesce_mutations_for_push(db::get_pending_mutations(&conn)?), reading_debounce, std::time::SystemTime::now())?
        };
        let session = self.current_session().ok_or("not signed in")?;
        let (mut remaining, set_aside) = partition_transmittable(changes)?;
        let mut cursor = starting_cursor;
        let mut sent_page = false;
        let mut transient_rejections = Vec::new();
        let mut assets_needed = false;
        for mutation_id in &set_aside {
            log::error!("Local sync mutation {mutation_id} encodes to more than the {} byte push limit and cannot be transmitted; it stays queued while every other change continues to synchronize", sync_common::MAX_PUSH_MUTATION_BYTES);
        }
        transient_rejections.extend(set_aside.into_iter().map(|mutation_id| sync_common::MutationRejection { mutation_id, reason: sync_common::MutationRejectionReason::InvalidPayload }));
        while !remaining.is_empty() || !sent_page {
            let requested = take_push_page(&mut remaining);
            let requested_ids = requested.iter().map(|pending| pending.mutation.mutation_id).collect::<Vec<_>>();
            // Captured before the request consumes the page; a lifecycle or
            // placement mutation the server accepts may need its bytes moved.
            let asset_bearing = requested.iter().filter(|pending| matches!(pending.mutation.body, MutationBody::BookLifecycle { .. } | MutationBody::Placement { .. })).map(|pending| pending.mutation.mutation_id).collect::<Vec<_>>();
            let wire_requested = requested.into_iter().map(|pending| pending.wire).collect::<Vec<_>>();
            let response = self.handle_client_result(sync_client::exchange_changes(&self.http_client, &session, self.library_id.clone(), self.replica_id.clone(), wire_requested, cursor).await)?;
            assets_needed |= asset_bearing.iter().any(|mutation_id| response.push.accepted.contains(mutation_id));
            transient_rejections.extend(self.apply_push_response(&requested_ids, &response.push)?);
            let (next_cursor, page_assets_needed) = self.pull_events_from_server_with_first(cursor, Some(response.pull)).await?;
            cursor = next_cursor;
            assets_needed |= page_assets_needed;
            sent_page = true;
        }
        Ok((reading_delay, assets_needed, transient_rejections))
    }

    async fn pull_events_from_server(&self, cursor: sync_common::SyncCursor) -> Result<(), Box<dyn std::error::Error>> {
        self.pull_events_from_server_with_first(cursor, None).await.map(|_| ())
    }

    async fn pull_events_from_server_with_first(&self, starting_cursor: sync_common::SyncCursor, mut first_response: Option<PullStateResponse>) -> Result<(sync_common::SyncCursor, bool), Box<dyn std::error::Error>> {
        let session = self.current_session().ok_or("not signed in")?;
        let mut cursor = starting_cursor;
        let mut assets_needed = false;
        loop {
            let pull_response = match first_response.take() {
                Some(response) => response,
                None => self.handle_client_result(sync_client::exchange_changes(&self.http_client, &session, self.library_id.clone(), self.replica_id.clone(), Vec::new(), cursor).await)?.pull,
            };
            let has_more = pull_response.has_more;
            validate_pull_batch(&pull_response, cursor)?;
            let next_cursor = pull_response.next_cursor;
            let changes = pull_response.mutations;
            assets_needed |= changes.iter().any(|change| matches!(change.mutation.kind.as_str(), "book_lifecycle" | "placement"));

            if changes.is_empty() {
                if has_more {
                    return Err("server returned an empty synchronization page with has_more=true".into());
                }
                if next_cursor != cursor {
                    let conn = self.db_pool.get()?;
                    let tx = conn.unchecked_transaction()?;
                    db::set_pull_cursor(&tx, next_cursor)?;
                    tx.commit()?;
                }
                cursor = next_cursor;
                break;
            }

            let mut conn = self.db_pool.get()?;
            let tx = conn.transaction()?;
            // Pull pages are applied in server revision order and the exact
            // advertised cursor advances only in the same commit. A placement
            // or child directory can precede its dependency across page boundaries;
            // the reducer materializes hidden placeholders until those dependencies
            // arrive.
            tx.pragma_update(None, "defer_foreign_keys", "ON")?;
            let mut removed_placements = Vec::new();
            let dirs_before = db::get_live_dir_paths_tx(&tx)?;
            for change in &changes {
                if let Some(removed) = db::apply_remote_change_in_tx(&tx, change)? {
                    removed_placements.push(removed);
                }
            }
            let dirs_after = db::get_live_dir_paths_tx(&tx)?;

            // A removal that won the LWW gate also acts on the local file, so a delete
            // on one device propagates to the others (otherwise the next scan would
            // see the still-present file and resurrect the placement). A move arrives
            // as add(new) then remove(old) — the grace period guarantees the add lands
            // first — so we relocate rather than delete + re-download.
            //
            // These effects are recorded in the same commit that advances the
            // cursor, because the cursor moving past work the disk never
            // received is exactly what resurrects a deleted book.
            let mut owed = Vec::new();
            owed.extend(dirs_after.iter().filter_map(|(id, new_path)| match dirs_before.get(id) {
                None => Some(db::FsAction::MaterializeDir { dir_id: id.to_string() }),
                Some(old_path) if old_path != new_path => Some(db::FsAction::MoveDir { dir_id: id.to_string(), old_path: old_path.as_str().to_owned(), new_path: new_path.as_str().to_owned() }),
                Some(_) => None,
            }));
            owed.extend(removed_placements.iter().map(|placement| db::FsAction::RetirePlacementFile { dir_id: placement.dir_id.to_string(), content_hash: placement.content_hash.to_string() }));
            // Deepest first, so a parent is never asked to vanish while a child
            // it still contains is queued behind it.
            let mut dir_deletes: Vec<_> = dirs_before.iter().filter(|(id, _)| !dirs_after.contains_key(*id)).map(|(_, old_path)| old_path.clone()).collect();
            dir_deletes.sort_by_key(|path| std::cmp::Reverse(path.as_path().components().count()));
            owed.extend(dir_deletes.iter().map(|path| db::FsAction::RemoveDir { path: path.as_str().to_owned() }));

            db::enqueue_fs_actions(&tx, &owed)?;
            db::set_pull_cursor(&tx, next_cursor)?;

            tx.commit()?;
            drop(conn);

            self.run_owed_fs_actions();
            cursor = next_cursor;
            if !has_more {
                break;
            }
        }
        Ok((cursor, assets_needed))
    }

    /// Performs the filesystem work recorded by applied pull pages, retiring
    /// each entry as it completes.
    ///
    /// Every action re-derives what it needs from committed state and tolerates
    /// having already been performed, so replaying whatever a crash left behind
    /// is safe. An action that genuinely fails (a directory a user dropped
    /// files into, say) is logged and retired rather than retried forever: the
    /// log exists to survive a crash, not to make the filesystem obey.
    pub(crate) fn run_owed_fs_actions(&self) {
        let Ok(conn) = self.db_pool.get() else {
            return;
        };
        let owed = match db::pending_fs_actions(&conn) {
            Ok(owed) => owed,
            Err(error) => {
                log::warn!("Could not read the pending filesystem actions: {error}");
                return;
            }
        };
        for (id, action) in owed {
            match action {
                Some(action) => {
                    if let Err(error) = self.run_fs_action(&conn, &action) {
                        log::warn!("Synchronized filesystem action {action:?} did not complete: {error}");
                    }
                }
                None => log::error!("Discarding an unreadable pending filesystem action so the work queued behind it can proceed"),
            }
            if let Err(error) = db::complete_fs_action(&conn, id) {
                log::warn!("Could not retire a completed filesystem action: {error}");
                return;
            }
        }
    }

    fn run_fs_action(&self, conn: &rusqlite::Connection, action: &db::FsAction) -> Result<(), Box<dyn std::error::Error>> {
        match action {
            db::FsAction::MaterializeDir { dir_id } => {
                DirId::parse_str(dir_id)?;
                db::materialize_dir_with_markers(conn, self.library_root.as_path(), dir_id)?;
                Ok(())
            }
            db::FsAction::MoveDir { dir_id, old_path, new_path } => self.move_remote_dir(conn, dir_id, old_path, new_path),
            db::FsAction::RetirePlacementFile { dir_id, content_hash } => self.retire_placement_file(conn, &DirId::parse_str(dir_id)?, &content_hash.parse::<ContentHash>()?),
            db::FsAction::RemoveDir { path } => {
                self.remove_remote_dir(&RelativeDirPath::parse(path)?);
                Ok(())
            }
        }
    }

    fn move_remote_dir(&self, conn: &rusqlite::Connection, dir_id: &str, old_path: &str, new_path: &str) -> Result<(), Box<dyn std::error::Error>> {
        DirId::parse_str(dir_id)?;
        let library_root = &self.library_root;
        let old_abs = RelativeDirPath::parse(old_path)?.resolve_under(library_root);
        let new_abs = RelativeDirPath::parse(new_path)?.resolve_under(library_root);
        // Already moved (or never materialized): create it where it belongs now.
        if !old_abs.exists() {
            db::materialize_dir_with_markers(conn, library_root.as_path(), dir_id)?;
            return Ok(());
        }
        db::materialize_dir_parent_with_markers(conn, library_root.as_path(), dir_id)?;
        std::fs::rename(&old_abs, &new_abs)?;
        db::materialize_dir_with_markers(conn, library_root.as_path(), dir_id)?;
        Ok(())
    }

    fn remove_remote_dir(&self, old_path: &RelativeDirPath) {
        let abs = old_path.resolve_under(&self.library_root);
        let marker = abs.join(".biblos_uuid");
        // Read the marker before touching anything so we can restore it if
        // remove_dir fails due to stray user files (leaving marker intact
        // preserves the dir identity for the next scanner pass).
        let marker_content = std::fs::read(&marker).ok();
        if marker_content.is_some() {
            match std::fs::remove_file(&marker) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => log::warn!("Failed to remove marker {:?}: {}", marker, err),
            }
        }
        match std::fs::remove_dir(&abs) {
            Ok(()) => log::info!("Removed remote-deleted dir {:?}", abs),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                log::warn!("Failed to remove remote-deleted dir {:?}: {}", abs, err);
                if let Some(content) = marker_content {
                    let _ = std::fs::write(&marker, content);
                }
            }
        }
    }

    fn retire_placement_file(&self, conn: &rusqlite::Connection, dir_id: &DirId, placement_hash: &ContentHash) -> Result<(), Box<dyn std::error::Error>> {
        {
            // Only act if still tombstoned in the committed state; a later add in
            // the same batch may have resurrected this exact placement.
            let Some((rel_path, content_hash)) = db::resolve_removed_book_dir(conn, &dir_id.to_string(), placement_hash.clone())? else {
                return Ok(());
            };
            let abs_old = self.resolve_book_path(&rel_path);
            if !Path::new(&abs_old).exists() {
                return Ok(());
            }

            // Move, not delete: if a live placement holds the same content but is
            // missing its file (the book was moved to a new path), relocate the
            // file there instead of deleting and later re-downloading it.
            if let Some(content_hash) = content_hash {
                if let Some(target) = self.live_placement_missing_file(conn, &content_hash)? {
                    let target_rel = target.rel_path;
                    let abs_new = self.resolve_book_path(&target_rel);
                    let mut target_ready = true;
                    if let Some((_, dir_id, _)) = db::get_book_dir_entry_by_path(conn, &target_rel)? {
                        if let Err(err) = db::materialize_dir_with_markers(conn, self.library_root.as_path(), &dir_id.to_string()) {
                            log::warn!("Failed to materialize relocation target {}: {}", target_rel, err);
                            target_ready = false;
                        }
                    } else if let Some(parent) = Path::new(&abs_new).parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if target_ready && std::fs::rename(&abs_old, &abs_new).is_ok() {
                        if let Err(err) = db::stamp_book_dir_local_hash_by_rel_path(conn, &target_rel, target.content_hash.clone(), Path::new(&abs_new)) {
                            log::warn!("Failed to stamp relocated fingerprint for {}: {}", abs_new, err);
                        }
                        db::upsert_blob_local_ref(conn, BlobKind::Book, &content_hash, &abs_new, true)?;
                        db::complete_asset_request(conn, BlobKind::Book, &content_hash)?;
                        db::set_book_downloaded(conn, &target_rel, target.content_hash, true)?;
                        log::info!("Relocated {abs_old} -> {abs_new} (book moved)");
                        return Ok(());
                    }
                }
            }

            match std::fs::remove_file(&abs_old) {
                Ok(()) => log::info!("Removed {abs_old} after synchronized deletion"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => log::warn!("Failed to remove {abs_old} after synchronized deletion: {error}"),
            }
        }
        Ok(())
    }

    /// Library-relative path of a live placement of `content_hash` whose file is
    /// not on disk — a relocation target for a moved book's file.
    fn live_placement_missing_file(&self, conn: &rusqlite::Connection, content_hash: &ContentHash) -> Result<Option<db::BookPath>, rusqlite::Error> {
        Ok(db::get_book_paths_by_content_hash(conn, content_hash)?.into_iter().find(|path| !Path::new(&self.resolve_book_path(&path.rel_path)).exists()))
    }
}

fn should_download_thumbnail(book_is_local: bool, thumbnail_is_local: bool, thumbnail_is_known_remote: bool, thumbnail_generation_pending: bool) -> bool {
    !thumbnail_is_local && !thumbnail_generation_pending && (!book_is_local || thumbnail_is_known_remote)
}

fn cleanup_stale_transfer_temps(root: &Path, cutoff: std::time::SystemTime) {
    for entry in walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str() else { continue };
        let Some(raw_uuid) = name.rsplit_once(".tmp-").map(|(_, suffix)| suffix) else { continue };
        if uuid::Uuid::parse_str(raw_uuid).is_err() {
            continue;
        }
        let old_enough = entry.metadata().ok().and_then(|metadata| metadata.modified().ok()).is_some_and(|modified| modified <= cutoff);
        if old_enough {
            if let Err(error) = std::fs::remove_file(entry.path()) {
                log::warn!("Failed to remove stale transfer temp {}: {error}", entry.path().display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_model::{AnnotationAnchor, AnnotationState, AnnotationStyle, BookFormat, BookLifecycleState, BookMetadataState, ReadingPositionState};
    use library_replica::{MutationBody, StateMutation};
    use r2d2_sqlite::SqliteConnectionManager;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Child, Command};
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
        *manager.session.write().unwrap() = Some(crate::auth::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        {
            let conn = manager.db_pool.get().unwrap();
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
        assert!(db::get_pending_mutations(&manager.db_pool.get().unwrap()).unwrap().is_empty());
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
        *manager.session.write().unwrap() = Some(crate::auth::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());

        manager.pull_events_from_server(SyncCursor::default()).await.unwrap();

        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        let conn = manager.db_pool.get().unwrap();
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
        *manager.session.write().unwrap() = Some(crate::auth::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());

        manager.pull_events_from_server(SyncCursor::default()).await.unwrap();

        let conn = manager.db_pool.get().unwrap();
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
        *manager.session.write().unwrap() = Some(crate::auth::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        {
            let conn = manager.db_pool.get().unwrap();
            db::set_sync_metadata(&conn, "last_pull_server_seq", "10").unwrap();
            assert_eq!(db::get_pull_cursor(&conn).unwrap(), cursor(LibraryRevision::new(10).ok()));
        }

        manager.pull_events_from_server(cursor(LibraryRevision::new(10).ok())).await.unwrap();

        let conn = manager.db_pool.get().unwrap();
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
        *manager.session.write().unwrap() = Some(crate::auth::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());

        manager.exchange_events_with_server(SyncCursor::default(), None).await.unwrap();

        assert_eq!(state.exchanges.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(state.pulls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let conn = manager.db_pool.get().unwrap();
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
        *manager.session.write().unwrap() = Some(crate::auth::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        {
            let mut conn = manager.db_pool.get().unwrap();
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

        let (_, _, deferred) = manager.exchange_events_with_server(SyncCursor::default(), None).await.expect("a deferred page still completes the exchange");

        assert_eq!(state.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        let rejected = state.rejected.lock().unwrap().unwrap();
        assert_eq!(deferred, vec![MutationRejection { mutation_id: rejected, reason: MutationRejectionReason::MissingDependency }]);
        let conn = manager.db_pool.get().unwrap();
        let pending = db::get_pending_mutations(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].mutation_id, rejected);
        server.abort();
    }

    fn queue_outbox_reading_change(manager: &SyncManager, mutation: &StateMutation, sequence: u64) {
        let conn = manager.db_pool.get().unwrap();
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
        *manager.session.write().unwrap() = Some(crate::auth::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        for sequence in 1..=2 {
            queue_outbox_reading_change(&manager, &reading_change(sequence, sequence, "epubcfi(/6/2)", sequence), sequence);
        }

        let outcome = manager.synchronize(SyncPolicy { assets: AssetSync::Transfer(TransferOrigin::Background), reading_positions: ReadingPositionSync::Immediate }).await;

        // Side effects first, so this test reports what the exchange actually
        // did before it reports how it answered.
        let conn = manager.db_pool.get().unwrap();
        let pending = db::get_pending_mutations(&conn).unwrap();
        assert_eq!(pending.len(), 1, "the deferred mutation stays queued for a later attempt");
        assert_eq!(pending[0].mutation_id, state.deferred.lock().unwrap().unwrap());
        assert_eq!(db::get_pull_cursor(&conn).unwrap(), cursor(LibraryRevision::new(1).ok()), "the pulled page is still applied");

        let outcome = outcome.expect("one deferred mutation must not fail the synchronization");
        assert_eq!(outcome.deferred_mutations, vec![MutationRejection { mutation_id: state.deferred.lock().unwrap().unwrap(), reason: MutationRejectionReason::TimestampTooFarFuture }]);
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
        *manager.session.write().unwrap() = Some(crate::auth::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        {
            let conn = manager.db_pool.get().unwrap();
            db::set_pull_cursor(&conn, cursor(LibraryRevision::new(99).ok())).unwrap();
        }

        manager.sync_once().await.expect("a rejected cursor must be recoverable without operator intervention");

        assert_eq!(state.calls.load(std::sync::atomic::Ordering::SeqCst), 3, "recovery performs one clean pull before the one-shot publishing retry");
        let conn = manager.db_pool.get().unwrap();
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
            let conn = manager.db_pool.get().unwrap();
            db::set_pull_cursor(&conn, cursor(LibraryRevision::new(99).ok())).unwrap();
        }

        manager.begin_cursor_recovery().unwrap();

        let conn = manager.db_pool.get().unwrap();
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
        *manager.session.write().unwrap() = Some(crate::auth::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());
        {
            let conn = manager.db_pool.get().unwrap();
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
        assert_eq!(db::get_sync_metadata(&manager.db_pool.get().unwrap(), CURSOR_RECOVERY_PENDING_KEY).unwrap(), None, "successful recovery clears its durable marker");
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

    fn transmittable(changes: Vec<StateMutation>) -> VecDeque<PendingPush> {
        let (queued, set_aside) = partition_transmittable(changes).unwrap();
        assert!(set_aside.is_empty(), "this fixture holds nothing larger than a page");
        queued
    }

    #[test]
    fn push_pages_are_bounded_by_count_and_preserve_sequence_order() {
        let mut changes = transmittable((1..=sync_common::MAX_PUSH_MUTATIONS as u64 + 7).map(|sequence| reading_change(sequence, sequence, "epubcfi(/6/2)", sequence)).collect::<Vec<_>>());

        let first = take_push_page(&mut changes);
        let second = take_push_page(&mut changes);

        assert_eq!(first.len(), sync_common::MAX_PUSH_MUTATIONS);
        assert_eq!(second.len(), 7);
        assert_eq!(first.first().unwrap().mutation.replica_seq.get(), 1);
        assert_eq!(second.first().unwrap().mutation.replica_seq.get(), sync_common::MAX_PUSH_MUTATIONS as u64 + 1);
    }

    /// The service refuses a request larger than its decoded request limit, and
    /// no batching makes an over-limit mutation fit. Left at the head of the
    /// outbox it fails every exchange, so each retry blocks every later
    /// mutation behind it. It must be set aside without being acknowledged:
    /// dropping it would leave this replica silently diverged from the server.
    #[test]
    fn a_mutation_too_large_to_transmit_does_not_block_the_rest_of_the_outbox() {
        let ahead = reading_change(1, 1, "epubcfi(/6/2)", 1);
        let oversized = annotation_change(2, "x".repeat(sync_common::MAX_PUSH_MUTATION_BYTES + 1));
        let behind = reading_change(3, 3, "epubcfi(/6/4)", 3);

        let (mut queued, set_aside) = partition_transmittable(vec![ahead.clone(), oversized.clone(), behind.clone()]).unwrap();

        assert_eq!(set_aside, vec![oversized.mutation_id], "only the untransmittable mutation is set aside");
        let page = take_push_page(&mut queued);
        assert_eq!(page.iter().map(|pending| pending.mutation.mutation_id).collect::<Vec<_>>(), vec![ahead.mutation_id, behind.mutation_id], "the mutations queued behind it still go out, in sequence order");
        assert!(queued.is_empty(), "the exchange loop has nothing left to spin on");
    }

    #[test]
    fn push_pages_stop_before_the_encoded_byte_target() {
        let large_text = "x".repeat(64 * 1024);
        let mut queued = transmittable((1..=100_u64).map(|sequence| annotation_change(sequence, large_text.clone())).collect::<Vec<_>>());

        let page = take_push_page(&mut queued);
        // The budget is spent in transmitted bytes, so the page is measured in
        // the same wire encoding the request carries.
        let encoded_bytes = page.iter().map(|pending| pending.encoded_bytes).sum::<usize>();

        assert!(!page.is_empty());
        assert!(page.len() < 100);
        assert!(encoded_bytes <= sync_common::MAX_PUSH_MUTATION_BYTES);
        assert_eq!(encoded_bytes, page.iter().map(|pending| sync_common::transport::encode(&pending.wire).unwrap().len()).sum::<usize>());
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
        let now = std::time::UNIX_EPOCH + Duration::from_millis(100_000);

        let (ready, remaining) = mutations_ready_for_push(changes, Some(Duration::from_secs(30)), now).unwrap();

        assert_eq!(ready.len(), 1);
        assert!(matches!(ready[0].body, MutationBody::BookLifecycle { .. }));
        assert_eq!(remaining, Some(Duration::from_secs(25)));
    }

    #[test]
    fn reading_upload_is_released_after_the_quiet_period() {
        let changes = vec![reading_change(1, 1, "epubcfi(/6/6)", 70_000)];
        let now = std::time::UNIX_EPOCH + Duration::from_millis(100_000);

        let (ready, remaining) = mutations_ready_for_push(changes, Some(Duration::from_secs(30)), now).unwrap();

        assert_eq!(ready.len(), 1);
        assert!(remaining.is_none());
    }

    #[test]
    fn pathological_future_reading_timestamp_is_rejected_instead_of_scheduling_an_unbounded_delay() {
        let now_millis = 100_000;
        let changes = vec![reading_change(1, 1, "epubcfi(/6/8)", now_millis + sync_common::MAX_LWW_FUTURE_SKEW_MS + 1)];
        let now = std::time::UNIX_EPOCH + Duration::from_millis(now_millis);

        let error = mutations_ready_for_push(changes, Some(Duration::from_secs(30)), now).unwrap_err();

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
        assert_eq!(validated.transient_rejections, vec![(changes[0].mutation_id, MutationRejectionReason::TimestampTooFarFuture)]);
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
        assert_eq!(validate_push_response(&requested_ids, &response).unwrap().permanent_rejections, vec![(requested[0].mutation_id, MutationRejectionReason::InvalidPayload)]);
    }

    #[test]
    fn permanent_rejection_removes_poison_mutation_from_durable_outbox() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let requested = reading_change(1, 1, "epubcfi(/6/24)", 1);
        {
            let conn = manager.db_pool.get().unwrap();
            conn.execute(
                "INSERT INTO sync_outbox(replica_seq,mutation_id,state_kind,state_key,body,changed_at) VALUES(?1,?2,'reading_position','1',?3,?4)",
                rusqlite::params![requested.replica_seq.get() as i64, requested.mutation_id.to_string(), sync_common::wire::encode(&requested.body).unwrap(), i64::try_from(requested.changed_at).unwrap()],
            )
            .unwrap();
        }
        let response = PushMutationsResponse { accepted: Vec::new(), rejected: vec![MutationRejection { mutation_id: requested.mutation_id, reason: MutationRejectionReason::InvalidPayload }] };

        assert!(manager.apply_push_response(&[requested.mutation_id], &response).is_err());

        let conn = manager.db_pool.get().unwrap();
        assert!(db::get_pending_mutations(&conn).unwrap().is_empty());
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
        *manager.session.write().unwrap() = Some(crate::auth::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap());

        // A live placement whose file is on disk, which the pull then removes.
        // Seeded as remote-origin state: a local-origin insert would stamp the
        // cell with the current wall clock and the pulled removal would lose
        // the merge, which is a property of the reducer, not of this test.
        let dir_id = fixture_dir_id("Shelf");
        let content_hash = fixture_content_hash(1);
        {
            let conn = manager.db_pool.get().unwrap();
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
        let conn = manager.db_pool.get().unwrap();
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
        let conn = manager.db_pool.get().unwrap();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES (?1,'00000000-0000-0000-0000-000000000000',?2)", rusqlite::params![dir_id.to_string(), dir]).unwrap();
        conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [content_hash.as_str()]).unwrap();
        conn.execute(
            "INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,deleted_at) VALUES (?2,(SELECT row_id FROM book WHERE content_hash=?1),?3,'fp',1,1)",
            rusqlite::params![content_hash.as_str(), dir_id.to_string(), file_name],
        )
        .unwrap();
        let on_disk = manager.library_root.join(dir).join(file_name);
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
            let conn = manager.db_pool.get().unwrap();
            let tx = conn.unchecked_transaction().unwrap();
            db::enqueue_fs_actions(&tx, &[db::FsAction::RetirePlacementFile { dir_id: dir_id.to_string(), content_hash: content_hash.to_string() }]).unwrap();
            tx.commit().unwrap();
        }
        assert!(on_disk.exists(), "the crash left the deletion unperformed");

        manager.run_owed_fs_actions();

        assert!(!on_disk.exists(), "the next run must still owe, and perform, the synchronized deletion");
        let conn = manager.db_pool.get().unwrap();
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
            let conn = manager.db_pool.get().unwrap();
            let tx = conn.unchecked_transaction().unwrap();
            db::enqueue_fs_actions(&tx, std::slice::from_ref(&action)).unwrap();
            tx.commit().unwrap();
            drop(conn);
            manager.run_owed_fs_actions();
        }

        assert!(!on_disk.exists());
        assert!(db::pending_fs_actions(&manager.db_pool.get().unwrap()).unwrap().is_empty());
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
            let conn = manager.db_pool.get().unwrap();
            conn.execute("INSERT INTO pending_fs_action(action, created_at) VALUES(?1, 1)", [b"not a decodable action".to_vec()]).unwrap();
            let tx = conn.unchecked_transaction().unwrap();
            db::enqueue_fs_actions(&tx, &[db::FsAction::RetirePlacementFile { dir_id: dir_id.to_string(), content_hash: content_hash.to_string() }]).unwrap();
            tx.commit().unwrap();
        }

        manager.run_owed_fs_actions();

        assert!(!on_disk.exists(), "work queued behind an unreadable row still runs");
        assert!(db::pending_fs_actions(&manager.db_pool.get().unwrap()).unwrap().is_empty());
    }

    /// Directory removals are recorded deepest first, so a parent is never
    /// asked to vanish while a child it still contains is queued behind it.
    #[test]
    fn owed_directory_removals_are_recorded_deepest_first() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let parent = manager.library_root.join("Shelf");
        let child = parent.join("Sub");
        std::fs::create_dir_all(&child).unwrap();

        {
            let conn = manager.db_pool.get().unwrap();
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
        let manager = SqliteConnectionManager::file(root.join(crate::APP_HIDDEN_LIBRARY_DB_PATH)).with_init(|conn| db::configure_connection(conn));
        let pool = Pool::builder().max_size(2).build(manager).unwrap();
        {
            let conn = pool.get().unwrap();
            db::init_db(&conn).unwrap();
        }
        SyncManager {
            db_pool: pool,
            replica_id: fixture_replica_id(replica_id),
            server_url: ServerUrl::parse("http://127.0.0.1:9").unwrap(),
            http_client: reqwest::Client::new(),
            library_id: fixture_library_id("library"),
            library_root: root.canonicalize().unwrap(),
            event_tx: None,
            session: Arc::new(std::sync::RwLock::new(None)),
            session_path: root.join(crate::APP_HIDDEN_SESSION_PATH),
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
        crate::auth::save_session_to(&session, &manager.session_path).unwrap();
        *manager.session.write().unwrap() = Some(session.clone());

        let conn = manager.db_pool.get().unwrap();
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
        let offline_session = crate::auth::Session::try_new(ServerUrl::parse("http://127.0.0.1:9").unwrap(), session.user_id().to_owned(), session.email().to_owned(), session.token().to_owned()).unwrap();
        crate::auth::save_session_to(&offline_session, &writer.session_path).unwrap();
        *writer.session.write().unwrap() = Some(offline_session);

        // The writer has no network route while it makes its local changes.
        // A failed exchange must leave the durable outbox intact for a later
        // reconnection, rather than losing either local register.
        let first_dir = fixture_dir_id("backend-e2e-offline-a");
        let second_dir = fixture_dir_id("backend-e2e-offline-b");
        {
            let conn = writer.db_pool.get().unwrap();
            let transaction = conn.unchecked_transaction().unwrap();
            db::upsert_dir(&transaction, &first_dir, &sync_common::ROOT_DIR_ID, &"Offline A".to_owned()).unwrap();
            db::upsert_dir(&transaction, &second_dir, &sync_common::ROOT_DIR_ID, &"Offline B".to_owned()).unwrap();
            transaction.commit().unwrap();
        }
        assert!(writer.sync_once().await.is_err(), "the unavailable server must fail the sync attempt");
        assert!(writer.sync_once().await.is_err(), "a repeated offline attempt must not consume the durable outbox");
        {
            let conn = writer.db_pool.get().unwrap();
            assert_eq!(db::get_pending_mutations(&conn).unwrap().len(), 6, "all three registers for each directory remain queued while offline");
        }

        writer.server_url = server_url.clone();
        crate::auth::save_session_to(&session, &writer.session_path).unwrap();
        *writer.session.write().unwrap() = Some(session.clone());
        writer.sync_once().await.expect("reconnect and publish offline outbox");
        {
            let conn = writer.db_pool.get().unwrap();
            assert!(db::get_pending_mutations(&conn).unwrap().is_empty(), "server acknowledgement clears only the published outbox");
        }

        let observer_root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(observer_root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut observer = test_manager_with_replica(observer_root.path(), "backend-e2e-offline-observer");
        observer.server_url = server_url;
        observer.library_id = library_id;
        crate::auth::save_session_to(&session, &observer.session_path).unwrap();
        *observer.session.write().unwrap() = Some(session);
        observer.sync_once().await.expect("second backend pulls the writer's offline history");

        let conn = observer.db_pool.get().unwrap();
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
        crate::auth::save_session_to(&session, &observer.session_path).unwrap();
        *observer.session.write().unwrap() = Some(session);
        observer.sync_once().await.expect("drain every remote synchronization page");
        let conn = observer.db_pool.get().unwrap();
        let positions: i64 = conn.query_row("SELECT COUNT(*) FROM book WHERE read_pos IS NOT NULL", [], |row| row.get(0)).unwrap();
        assert_eq!(positions, sync_common::MAX_PULL_CHANGES as i64 + 1);
        let cursor = db::get_pull_cursor(&conn).unwrap();
        assert!(cursor.reading_revision.is_some(), "the final reading cursor is persisted with the final page");
    }

    #[tokio::test]
    #[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
    async fn production_http_postgres_serves_assets_and_notification_endpoints() {
        let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL")).expect("valid E2E server URL");
        let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
        let observer_session = sync_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in observer");
        let publisher_session = sync_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in publisher");
        assert_eq!(sync_client::current_account(&observer_session).await.unwrap().email, observer_session.email());

        // A second credential must receive the publisher's change notification;
        // the publisher's own credential is intentionally filtered by the hub.
        let mut notifications = sync_client::connect_notifications(&observer_session).await.expect("connect authenticated notifications");
        assert!(matches!(tokio::time::timeout(Duration::from_secs(2), notifications.next()).await.unwrap().unwrap(), Some(sync_common::api::notifications::NotificationMessage::Hello { .. })));

        let library_id = fixture_library_id("backend-e2e-assets");
        sync_client::upsert_library(&publisher_session, library_id.clone(), "Backend asset E2E".to_owned()).await.expect("create E2E library");
        assert_eq!(tokio::time::timeout(Duration::from_secs(2), notifications.next()).await.unwrap().unwrap(), Some(sync_common::api::notifications::NotificationMessage::CatalogueChanged));
        notifications.close().await;
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
        let session = crate::auth::Session::try_new_with_expiry(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string(), Some(now + 60)).unwrap();
        crate::auth::save_session_to(&session, &manager.session_path).unwrap();
        *manager.session.write().unwrap() = Some(session);

        manager.sync_once().await.unwrap();

        assert!(!manager.is_signed_in());
        assert!(!manager.session_path.exists());
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
        let session = crate::auth::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap();
        crate::auth::save_session_to(&session, &manager.session_path).unwrap();
        *manager.session.write().unwrap() = Some(session);

        let error = manager.sync_once().await.unwrap_err();

        assert!(error.downcast_ref::<sync_client::AuthError>().is_some_and(sync_client::AuthError::is_unauthorized));
        assert!(manager.is_signed_in());
        assert!(manager.session_path.exists());
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
            let conn = manager.db_pool.get().unwrap();
            conn.execute("INSERT INTO dir(id, parent_id, name) VALUES (?1, '00000000-0000-0000-0000-000000000000', 'Old')", [dir_id_text.as_str()]).unwrap();
        }
        std::fs::create_dir_all(temp.path().join("Old")).unwrap();
        std::fs::write(temp.path().join("Old").join(".biblos_uuid"), &dir_id_text).unwrap();

        {
            let conn = manager.db_pool.get().unwrap();
            conn.execute("UPDATE dir SET name='New' WHERE id=?1", [dir_id_text.as_str()]).unwrap();
        }
        manager.run_fs_action(&manager.db_pool.get().unwrap(), &db::FsAction::MoveDir { dir_id: dir_id.to_string(), old_path: "Old".to_owned(), new_path: "New".to_owned() }).expect("a synchronized directory move relocates the folder");

        assert!(!temp.path().join("Old").exists());
        assert_eq!(std::fs::read_to_string(temp.path().join("New").join(".biblos_uuid")).unwrap(), dir_id_text);

        let conn = manager.db_pool.get().unwrap();
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

        // Recorded deepest first, the order `pull_events_from_server_with_first`
        // enqueues them in.
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
        let conn = manager.db_pool.get().unwrap();
        let dir_id = fixture_dir_id("d1").to_string();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES (?1, '00000000-0000-0000-0000-000000000000', 'Shelf')", [dir_id.as_str()]).unwrap();
        conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [TEST_CONTENT_HASH]).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES (?2, (SELECT row_id FROM book WHERE content_hash=?1), 'book.epub', 'fp', 0)", rusqlite::params![TEST_CONTENT_HASH, dir_id])
            .unwrap();

        let job = manager.download_job(&conn, &test_content_hash(), TransferOrigin::UserInitiated).unwrap().expect("live placement should produce a download job");
        match job {
            TransferJob::DownloadBlob { content_hash, placement, origin } => {
                assert_eq!(content_hash, test_content_hash());
                assert_eq!(placement.content_hash, test_content_hash());
                assert_eq!(placement.rel_path.as_str(), "/Shelf/aaaaaaaaaaaa.epub");
                assert_eq!(origin, TransferOrigin::UserInitiated);
            }
            _ => panic!("expected blob download"),
        }
    }

    #[test]
    fn download_request_requires_and_resolves_a_live_placement() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        {
            let conn = manager.db_pool.get().unwrap();
            conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [TEST_CONTENT_HASH]).unwrap();
        }

        assert!(manager.request_download(test_content_hash()).is_err());

        {
            let conn = manager.db_pool.get().unwrap();
            conn.execute(
                "INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=?1), 'book.epub', 'fp', 0)",
                [TEST_CONTENT_HASH],
            )
            .unwrap();
        }
        manager.request_download(test_content_hash()).unwrap();

        let conn = manager.db_pool.get().unwrap();
        assert_eq!(db::get_asset_requests(&conn, BlobKind::Book).unwrap(), vec![(test_content_hash(), TransferOrigin::UserInitiated)]);
    }
}
