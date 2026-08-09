#![allow(clippy::too_many_arguments)]

use super::transfer::{TransferJob, TransferOrigin, TransferQueue, TransferRuntime, TransferWorker};
use crate::library::database as db;
use crate::platform::scanner::LibraryEvent;
use crate::{BlobKind, BookId, BookPlacement, ContentHash, RelativeBookPath, ServerUrl};
use crate::{LibraryRoot, RelativeDirPath};
use crossbeam_channel::Sender;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use sync_common::api::assets::{BlobManifestEntry, BlobManifestResponse, MAX_BLOB_MANIFEST_ENTRIES};
#[cfg(test)]
use sync_common::SyncExchangeResponse;
use sync_common::{ClientId, DirId, LibraryId, PullStateResponse, PushMutationsResponse};
use bokheim_domain::{MutationBody, StateMutation};

use bokheim_sync_client::{coalesce_mutations_for_push, validate_pull_batch, validate_push_response, PushResponseError};

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SyncOutcome {
    pub next_reading_upload_in: Option<Duration>,
}

fn mutations_ready_for_push(mut mutations: Vec<StateMutation>, reading_debounce: Option<Duration>, now: std::time::SystemTime) -> Result<(Vec<StateMutation>, Option<Duration>), Box<dyn std::error::Error>> {
    let Some(reading_debounce) = reading_debounce else {
        return Ok((mutations, None));
    };
    let latest_reading_change = mutations.iter().filter(|mutation| matches!(mutation.body, MutationBody::ReadingPosition { .. })).map(|mutation| mutation.changed_at).max();
    let Some(latest_reading_change) = latest_reading_change else {
        return Ok((mutations, None));
    };
    let now_millis = u64::try_from(now.duration_since(std::time::UNIX_EPOCH)?.as_millis())?;
    if latest_reading_change.get() > now_millis.saturating_add(sync_common::MAX_LWW_FUTURE_SKEW_MS) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "local reading-position timestamp is pathologically far in the future").into());
    }
    let debounce_millis = u64::try_from(reading_debounce.as_millis())?;
    let ready_at = latest_reading_change.saturating_add(debounce_millis);
    if ready_at.get() <= now_millis {
        return Ok((mutations, None));
    }
    mutations.retain(|mutation| !matches!(mutation.body, MutationBody::ReadingPosition { .. }));
    Ok((mutations, Some(Duration::from_millis(ready_at.get() - now_millis))))
}

fn take_push_page(mutations: &mut VecDeque<StateMutation>) -> Result<Vec<StateMutation>, sync_common::wire::WireError> {
    let mut page = Vec::with_capacity(sync_common::MAX_PUSH_MUTATIONS.min(mutations.len()));
    let mut encoded_bytes = 0_usize;
    while page.len() < sync_common::MAX_PUSH_MUTATIONS {
        let Some(next) = mutations.front() else { break };
        let next_bytes = sync_common::wire::encode(next)?.len();
        if !page.is_empty() && encoded_bytes.saturating_add(next_bytes) > sync_common::MAX_PUSH_MUTATION_BYTES {
            break;
        }
        encoded_bytes = encoded_bytes.saturating_add(next_bytes);
        page.push(mutations.pop_front().expect("the inspected mutation is still queued"));
    }
    Ok(page)
}

pub type DbPool = Pool<SqliteConnectionManager>;

#[derive(Debug)]
pub struct SyncManager {
    db_pool: DbPool,
    client_id: ClientId,
    server_url: ServerUrl,
    http_client: reqwest::Client,
    library_id: LibraryId,
    library_root: LibraryRoot,
    event_tx: Option<Sender<LibraryEvent>>,
    session: Arc<std::sync::RwLock<Option<crate::auth::Session>>>,
    session_path: PathBuf,
    transfer_runtime: TransferRuntime,
    transfer_queue: Arc<TransferQueue>,
    sync_gate: tokio::sync::Mutex<()>,
}

#[derive(Debug)]
enum DirUpsertMaterialization {
    Create { dir_id: DirId },
    Move { dir_id: DirId, old_path: RelativeDirPath, new_path: RelativeDirPath },
}

impl SyncManager {
    pub(crate) fn new(
        db_pool: DbPool, client_id: ClientId, server_url: ServerUrl, library_id: LibraryId, library_root: LibraryRoot, session_path: PathBuf, event_tx: Option<Sender<LibraryEvent>>, transfer_runtime: TransferRuntime,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let http_client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;
        Ok(Self {
            db_pool,
            client_id,
            server_url,
            http_client,
            library_id,
            library_root,
            event_tx,
            session: Arc::new(std::sync::RwLock::new(None)),
            session_path,
            transfer_runtime,
            transfer_queue: Arc::new(TransferQueue::default()),
            sync_gate: tokio::sync::Mutex::new(()),
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

    /// Binds this local library to the session's (server-issued) user and stores
    /// it. Rejects a session whose user differs from the one this library was
    /// previously bound to, so two accounts can't share one local library.
    pub fn adopt_session(&self, session: crate::auth::Session) -> Result<(), String> {
        if session.server_url() != &self.server_url {
            return Err(format!("session belongs to {}, but this library syncs with {}", session.server_url(), self.server_url));
        }
        {
            let conn = self.db_pool.get().map_err(|e| e.to_string())?;
            match db::get_sync_metadata(&conn, "bound_user_id").map_err(|error| error.to_string())? {
                Some(bound) if bound != session.user_id() => {
                    return Err(format!(
                        "This library is already linked to a different account ({bound}). \
                         Logging in as {} here would mix two accounts.",
                        session.user_id()
                    ));
                }
                Some(_) => {}
                None => db::set_sync_metadata(&conn, "bound_user_id", session.user_id()).map_err(|error| error.to_string())?,
            }
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
        let reading_debounce = match policy.reading_positions {
            #[cfg(test)]
            ReadingPositionSync::Immediate => None,
            ReadingPositionSync::AfterQuietPeriod(duration) => Some(duration),
        };
        let (signed_in, reading_delay, assets_needed) = self.sync_state_under_gate(reading_debounce).await?;
        if !signed_in {
            return Ok(SyncOutcome::default());
        }
        if let AssetSync::Transfer(origin) = policy.assets {
            self.sync_assets(origin).await?;
        } else if assets_needed {
            self.sync_assets(TransferOrigin::Background).await?;
        }
        Ok(SyncOutcome { next_reading_upload_in: reading_delay })
    }

    #[cfg(test)]
    async fn sync_once(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.synchronize(SyncPolicy { assets: AssetSync::Transfer(TransferOrigin::Background), reading_positions: ReadingPositionSync::Immediate }).await.map(|_| ())
    }

    async fn sync_state_under_gate(&self, reading_debounce: Option<Duration>) -> Result<(bool, Option<Duration>, bool), Box<dyn std::error::Error>> {
        if !self.is_signed_in() {
            // Local-only mode: nothing to push, nothing to pull.
            return Ok((false, None, false));
        }
        if self.current_session().is_some_and(|session| if session.refresh_token().is_some() { session.refresh_expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) } else { session.expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS) }) {
            self.clear_session().map_err(std::io::Error::other)?;
            log::warn!("Authentication credentials expired or are too close to expiry; sign in again before synchronizing");
            return Ok((false, None, false));
        }

        // Get last pull server_seq from metadata
        let last_pull_server_seq = {
            let conn = self.db_pool.get()?;
            match db::get_sync_metadata(&conn, "last_pull_server_seq")? {
                Some(value) => Some(sync_common::LibraryRevision::new(value.parse::<u64>()?)?),
                None => None,
            }
        };

        // One authenticated exchange publishes the local outbox and returns the
        // first incremental page from the resulting committed server snapshot.
        let (reading_delay, assets_needed) = self.exchange_events_with_server(last_pull_server_seq, reading_debounce).await?;
        Ok((true, reading_delay, assets_needed))
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

    fn handle_client_result<T>(&self, result: Result<T, bokheim_sync_client::AuthError>) -> Result<T, Box<dyn std::error::Error>> {
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

        // Pass 1: record current on-disk presence of every live placement, so
        // pass 2 can see all local copies before deciding what to materialize.
        for asset in &assets {
            let local_path = self.resolve_book_path(&asset.rel_path);
            let thumb_path = self.thumbnail_path_for_content_hash(&asset.content_hash);
            db::upsert_blob_local_ref(&conn, BlobKind::Book, &asset.content_hash, &local_path, Path::new(&local_path).exists())?;
            db::upsert_blob_local_ref(&conn, BlobKind::Thumbnail, &asset.content_hash, &thumb_path, Path::new(&thumb_path).exists())?;
            known_hashes.insert(asset.content_hash.clone());
        }

        // Pass 2: a live placement missing its file whose content already exists at
        // another local path (the book was moved/copied) is filled by a local copy
        // rather than a download — free, and it avoids the window where a moved
        // book's new path has no file yet. Copy (not move): the source is itself a
        // live placement that still needs its file; the old path is deleted later
        // when its own removal arrives.
        for asset in &assets {
            let local_path = self.resolve_book_path(&asset.rel_path);
            if Path::new(&local_path).exists() {
                continue;
            }
            let Some(src) = db::get_blob_local_path(&conn, BlobKind::Book, &asset.content_hash)? else {
                continue;
            };
            if src == local_path {
                continue;
            }
            if let Some((_, dir_id, _)) = db::get_book_dir_entry_by_path(&conn, &asset.rel_path)? {
                if let Err(err) = db::materialize_dir_with_markers(&conn, self.library_root.as_path(), dir_id.as_str()) {
                    log::warn!("Failed to materialize parent dir for {}: {}", asset.rel_path, err);
                    continue;
                }
            }
            match self.library_root.copy_verified_book(Path::new(&src), &asset.rel_path, &asset.content_hash) {
                Ok(installed_path) => {
                    let Some(installed_path_text) = installed_path.to_str() else {
                        log::warn!("Verified local book path is not valid UTF-8: {}", installed_path.display());
                        continue;
                    };
                    if let Err(err) = db::stamp_book_dir_local_hash_by_rel_path(&conn, &asset.rel_path, asset.book_id, &installed_path) {
                        log::warn!("Failed to stamp local fingerprint for {}: {}", local_path, err);
                    }
                    db::upsert_blob_local_ref(&conn, BlobKind::Book, &asset.content_hash, installed_path_text, true)?;
                    db::complete_asset_request(&conn, BlobKind::Book, &asset.content_hash)?;
                    db::set_book_downloaded(&conn, &asset.rel_path, asset.book_id, true)?;
                    log::info!("Materialized {local_path} from local copy {src} (content already present)");
                }
                Err(error) => {
                    if error.invalidates_source() {
                        db::upsert_blob_local_ref(&conn, BlobKind::Book, &asset.content_hash, &src, false)?;
                        db::set_book_downloaded(&conn, &asset.rel_path, asset.book_id, false)?;
                        db::add_asset_request(&conn, BlobKind::Book, &asset.content_hash, TransferOrigin::Background)?;
                    }
                    log::warn!("Failed to materialize {local_path} from {src}: {error}");
                }
            }
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
            jobs.push(TransferJob::UploadThumbnail { content_hash, thumbnail_path, book_id: None, origin: upload_origin });
        }

        // A requested download is itself the authoritative presence check: a
        // successful GET records present and a 404 records missing.
        for (content_hash, origin) in pending_requests {
            let conn = self.db_pool.get()?;
            if db::is_blob_local(&conn, BlobKind::Book, &content_hash)? {
                db::complete_asset_request(&conn, BlobKind::Book, &content_hash)?;
                continue;
            }
            let Some(job) = self.download_job(&conn, &content_hash, origin)? else {
                continue;
            };
            jobs.push(job);
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
                if let Some(job) = self.download_job(&conn, &content_hash, TransferOrigin::Background)? {
                    jobs.push(job);
                }
            }
        }

        // Thumbnail GETs likewise record either present or missing themselves.
        for content_hash in known_hashes {
            let conn = self.db_pool.get()?;
            if db::is_blob_local(&conn, BlobKind::Thumbnail, &content_hash)? {
                continue;
            }
            let Some(book_path) = db::get_book_paths_by_content_hash(&conn, &content_hash)?.into_iter().next() else {
                continue;
            };
            jobs.push(TransferJob::DownloadThumbnail { content_hash, book_id: book_path.book_id, origin: upload_origin });
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
        .run(jobs, 4)
        .await?;
        Ok(())
    }

    pub fn transfer_statuses(&self) -> Vec<crate::TransferStatus> {
        self.transfer_queue.statuses()
    }

    pub fn request_download(&self, book_id: BookId) -> Result<(), Box<dyn std::error::Error>> {
        self.request_downloads(std::slice::from_ref(&book_id))
    }

    async fn remote_blob_size(&self, content_hash: &ContentHash) -> Result<Option<u64>, Box<dyn std::error::Error>> {
        let token = self.current_session().ok_or("authentication is required")?.token().to_owned();
        let endpoint = self.server_url.endpoint(&format!("api/libraries/{}/blobs/{}", urlencoding::encode(self.library_id.as_str()), urlencoding::encode(content_hash.as_str())));
        let response = self.http_client.head(endpoint).bearer_auth(token).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = response.error_for_status()?;
        Ok(response.content_length())
    }

    pub fn request_downloads(&self, book_ids: &[BookId]) -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = self.db_pool.get()?;
        let transaction = conn.transaction()?;
        for book_id in book_ids {
            let hashes = db::get_book_hashes(&transaction, *book_id)?;
            if hashes.is_empty() {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("book {book_id} has no live placement")).into());
            }
            for hash in hashes {
                db::add_asset_request(&transaction, BlobKind::Book, &hash, TransferOrigin::UserInitiated)?;
            }
        }
        transaction.commit()?;
        drop(conn);
        if let Some(tx) = &self.event_tx {
            for book_id in book_ids {
                let _ = tx.send(LibraryEvent::DownloadStatusChanged { book_id: *book_id, state: crate::DownloadState::Queued });
            }
        }
        Ok(())
    }

    fn resolve_book_path(&self, rel_path: &RelativeBookPath) -> String {
        self.library_root.resolve_book(rel_path).to_string_lossy().into_owned()
    }

    fn thumbnail_path_for_content_hash(&self, content_hash: &ContentHash) -> String {
        crate::sync::transfer::thumbnail_path_from_content_hash(&self.library_root, content_hash).to_string_lossy().into_owned()
    }

    fn download_job(&self, conn: &rusqlite::Connection, content_hash: &ContentHash, origin: TransferOrigin) -> Result<Option<TransferJob>, rusqlite::Error> {
        let book_paths = db::get_book_paths_by_content_hash(conn, content_hash)?;
        let Some(chosen) = book_paths.iter().find(|path| !Path::new(&self.resolve_book_path(&path.rel_path)).exists()).or_else(|| book_paths.first()) else {
            return Ok(None);
        };
        let materialized_path = db::materialize_download_path(conn, &self.library_root, &BookPlacement::new(chosen.book_id, chosen.rel_path.clone()))?;
        let local_path = self.resolve_book_path(&materialized_path);
        db::upsert_blob_local_ref(conn, BlobKind::Book, content_hash, &local_path, Path::new(&local_path).exists())?;
        Ok(Some(TransferJob::DownloadBlob { content_hash: content_hash.clone(), placement: BookPlacement::new(chosen.book_id, materialized_path), origin }))
    }

    async fn negotiate_upload_blobs(&self, blobs: Vec<BlobManifestEntry>) -> Result<BlobManifestResponse, Box<dyn std::error::Error>> {
        if blobs.is_empty() {
            return Ok(BlobManifestResponse::default());
        }
        let session = self.current_session().ok_or("not signed in")?;
        let mut combined = BlobManifestResponse::default();
        for chunk in blobs.chunks(MAX_BLOB_MANIFEST_ENTRIES) {
            let mut response = self.handle_client_result(bokheim_sync_client::negotiate_blobs(&self.http_client, &session, &self.library_id, chunk.to_vec()).await)?;
            combined.owned.append(&mut response.owned);
            combined.claimed.append(&mut response.claimed);
            combined.upload.append(&mut response.upload);
            combined.rejected.append(&mut response.rejected);
        }
        Ok(combined)
    }

    fn apply_push_response(&self, requested: &[StateMutation], push_response: &PushMutationsResponse) -> Result<Vec<sync_common::MutationId>, Box<dyn std::error::Error>> {
        let validated = validate_push_response(requested, push_response)?;

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
        Ok(validated.transient_rejections)
    }

    async fn exchange_events_with_server(&self, since_server_seq: Option<sync_common::LibraryRevision>, reading_debounce: Option<Duration>) -> Result<(Option<Duration>, bool), Box<dyn std::error::Error>> {
        let (changes, reading_delay) = {
            let conn = self.db_pool.get()?;
            mutations_ready_for_push(coalesce_mutations_for_push(db::get_pending_mutations(&conn)?), reading_debounce, std::time::SystemTime::now())?
        };
        let session = self.current_session().ok_or("not signed in")?;
        let mut remaining = VecDeque::from(changes);
        let mut cursor = since_server_seq;
        let mut sent_page = false;
        let mut transient_rejections = Vec::new();
        let mut assets_needed = false;
        while !remaining.is_empty() || !sent_page {
            let requested = take_push_page(&mut remaining)?;
            let response = self.handle_client_result(bokheim_sync_client::exchange_changes(&self.http_client, &session, self.library_id.clone(), self.client_id.clone(), requested.clone(), cursor).await)?;
            assets_needed |= requested.iter().any(|mutation| response.push.accepted.contains(&mutation.mutation_id) && matches!(mutation.body, MutationBody::BookLifecycle { .. } | MutationBody::Placement { .. }));
            transient_rejections.extend(self.apply_push_response(&requested, &response.push)?);
            let (next_cursor, page_assets_needed) = self.pull_events_from_server_with_first(cursor, Some(response.pull)).await?;
            cursor = next_cursor;
            assets_needed |= page_assets_needed;
            sent_page = true;
        }
        if !transient_rejections.is_empty() {
            return Err(PushResponseError::TransientRejection { mutation_ids: transient_rejections }.into());
        }
        Ok((reading_delay, assets_needed))
    }

    #[cfg(test)]
    async fn pull_events_from_server(&self, since_server_seq: Option<sync_common::LibraryRevision>) -> Result<(), Box<dyn std::error::Error>> {
        self.pull_events_from_server_with_first(since_server_seq, None).await.map(|_| ())
    }

    async fn pull_events_from_server_with_first(
        &self, since_server_seq: Option<sync_common::LibraryRevision>, mut first_response: Option<PullStateResponse>,
    ) -> Result<(Option<sync_common::LibraryRevision>, bool), Box<dyn std::error::Error>> {
        let session = self.current_session().ok_or("not signed in")?;
        let mut cursor = since_server_seq;
        let mut assets_needed = false;
        loop {
            let pull_response = match first_response.take() {
                Some(response) => response,
                None => self.handle_client_result(bokheim_sync_client::exchange_changes(&self.http_client, &session, self.library_id.clone(), self.client_id.clone(), Vec::new(), cursor).await)?.pull,
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
                    if let Some(next) = next_cursor {
                        let conn = self.db_pool.get()?;
                        db::set_sync_metadata(&conn, "last_pull_server_seq", &next.get().to_string())?;
                    }
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
            let dir_upserts: Vec<_> = dirs_after
                .iter()
                .filter_map(|(id, new_path)| match dirs_before.get(id) {
                    None => Some(DirUpsertMaterialization::Create { dir_id: id.clone() }),
                    Some(old_path) if old_path != new_path => Some(DirUpsertMaterialization::Move { dir_id: id.clone(), old_path: old_path.clone(), new_path: new_path.clone() }),
                    Some(_) => None,
                })
                .collect();
            let dir_deletes: Vec<_> = dirs_before.iter().filter(|(id, _)| !dirs_after.contains_key(*id)).map(|(_, old_path)| old_path.clone()).collect();

            if let Some(next) = next_cursor {
                db::set_sync_metadata(&tx, "last_pull_server_seq", &next.get().to_string())?;
            }

            tx.commit()?;
            drop(conn);

            self.materialize_remote_dir_upserts(&dir_upserts);
            // A removal that won the LWW gate also acts on the local file, so a delete
            // on one device propagates to the others (otherwise the next scan would
            // see the still-present file and resurrect the placement). A move arrives
            // as add(new) then remove(old) — the grace period guarantees the add lands
            // first — so we relocate rather than delete + re-download.
            self.apply_local_removals(&removed_placements)?;
            self.materialize_remote_dir_deletes(&dir_deletes);
            cursor = next_cursor;
            if !has_more {
                break;
            }
        }
        Ok((cursor, assets_needed))
    }

    fn materialize_remote_dir_upserts(&self, dirs: &[DirUpsertMaterialization]) {
        if dirs.is_empty() {
            return;
        }
        let Ok(conn) = self.db_pool.get() else {
            return;
        };
        let library_root = &self.library_root;
        for dir in dirs {
            match dir {
                DirUpsertMaterialization::Create { dir_id } => {
                    if let Err(err) = db::materialize_dir_with_markers(&conn, library_root.as_path(), dir_id.as_str()) {
                        log::warn!("Failed to materialize remote dir {}: {}", dir_id, err);
                    }
                }
                DirUpsertMaterialization::Move { dir_id, old_path, new_path } => {
                    let old_abs = library_root.resolve_dir(old_path);
                    let new_abs = library_root.resolve_dir(new_path);
                    if !old_abs.exists() {
                        if let Err(err) = db::materialize_dir_with_markers(&conn, library_root.as_path(), dir_id.as_str()) {
                            log::warn!("Failed to materialize moved remote dir {}: {}", dir_id, err);
                        }
                        continue;
                    }
                    if let Err(err) = db::materialize_dir_parent_with_markers(&conn, library_root.as_path(), dir_id.as_str()) {
                        log::warn!("Failed to materialize parent for moved dir {}: {}", dir_id, err);
                        continue;
                    }
                    match std::fs::rename(&old_abs, &new_abs) {
                        Ok(()) => {
                            if let Err(err) = db::materialize_dir_with_markers(&conn, library_root.as_path(), dir_id.as_str()) {
                                log::warn!("Moved dir {} but failed to verify marker: {}", dir_id, err);
                            }
                        }
                        Err(err) => log::warn!("Failed to rename remote dir {:?} -> {:?}: {}", old_abs, new_abs, err),
                    }
                }
            }
        }
    }

    fn materialize_remote_dir_deletes(&self, dirs: &[RelativeDirPath]) {
        let library_root = &self.library_root;
        let mut dirs: Vec<_> = dirs.iter().collect();
        dirs.sort_by_key(|path| std::cmp::Reverse(path.as_path().components().count()));
        for old_path in dirs {
            let abs = library_root.resolve_dir(old_path);
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
    }

    fn apply_local_removals(&self, removed: &[db::RemovedBookDir]) -> Result<(), Box<dyn std::error::Error>> {
        if removed.is_empty() {
            return Ok(());
        }
        let conn = self.db_pool.get()?;
        for placement in removed {
            // Only act if still tombstoned in the committed state; a later add in
            // the same batch may have resurrected this exact placement.
            let Some((rel_path, content_hash)) = db::resolve_removed_book_dir(&conn, placement.dir_id.as_str(), placement.book_id)? else {
                continue;
            };
            let abs_old = self.resolve_book_path(&rel_path);
            if !Path::new(&abs_old).exists() {
                continue;
            }

            // Move, not delete: if a live placement holds the same content but is
            // missing its file (the book was moved to a new path), relocate the
            // file there instead of deleting and later re-downloading it.
            if let Some(content_hash) = content_hash {
                if let Some(target) = self.live_placement_missing_file(&conn, &content_hash)? {
                    let target_rel = target.rel_path;
                    let abs_new = self.resolve_book_path(&target_rel);
                    if let Some((_, dir_id, _)) = db::get_book_dir_entry_by_path(&conn, &target_rel)? {
                        if let Err(err) = db::materialize_dir_with_markers(&conn, self.library_root.as_path(), dir_id.as_str()) {
                            log::warn!("Failed to materialize relocation target {}: {}", target_rel, err);
                            continue;
                        }
                    } else if let Some(parent) = Path::new(&abs_new).parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if std::fs::rename(&abs_old, &abs_new).is_ok() {
                        if let Err(err) = db::stamp_book_dir_local_hash_by_rel_path(&conn, &target_rel, target.book_id, Path::new(&abs_new)) {
                            log::warn!("Failed to stamp relocated fingerprint for {}: {}", abs_new, err);
                        }
                        db::upsert_blob_local_ref(&conn, BlobKind::Book, &content_hash, &abs_new, true)?;
                        db::complete_asset_request(&conn, BlobKind::Book, &content_hash)?;
                        db::set_book_downloaded(&conn, &target_rel, target.book_id, true)?;
                        log::info!("Relocated {abs_old} -> {abs_new} (book moved)");
                        continue;
                    }
                }
            }

            match bokheim_library_files::remove_managed_book_file(&self.library_root, Path::new(&abs_old), &placement.dir_id) {
                Ok(()) => log::info!("Removed {abs_old} after synchronized deletion"),
                Err(error) => log::warn!("Failed to remove {abs_old}; the scanner will retry: {error}"),
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
    use r2d2_sqlite::SqliteConnectionManager;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Child, Command};
    use bokheim_domain::{
        AnnotationAnchor, AnnotationState, AnnotationStyle, BookFormat, BookLifecycleState,
        BookMetadataState, MutationBody, ReadingPositionState, StateMutation,
    };
    use sync_common::{
        ClientId, FileName, LibraryRevision, MutationRejection, MutationRejectionReason,
        ReplicaSeq, ServerMutation,
    };

    const TEST_CONTENT_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const TEST_AUTH_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn test_content_hash() -> ContentHash {
        ContentHash::parse(TEST_CONTENT_HASH).unwrap()
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
        sync_common::UnixMillis::new(value).unwrap()
    }

    fn pulled_change(server_sequence: u64, body: MutationBody) -> ServerMutation {
        ServerMutation {
            mutation: StateMutation { mutation_id: sync_common::MutationId::new(), body, changed_at: millis(10), replica_seq: local_seq(server_sequence) }.to_wire().unwrap(),
            replica_id: ClientId::parse("remote-device").unwrap(),
            revision: LibraryRevision::new(server_sequence).unwrap(),
        }
    }

    fn deleted(book_id: u64) -> MutationBody {
        MutationBody::BookLifecycle { book_id: BookId::new(book_id), value: BookLifecycleState::Deleted }
    }

    fn added(book_id: u64, title: &str, added_at: u64) -> MutationBody {
        MutationBody::BookLifecycle {
            book_id: BookId::new(book_id),
            value: BookLifecycleState::Present {
                metadata: BookMetadataState { title: title.to_owned(), contributors: Vec::new(), description: String::new(), publication: Default::default() },
                added_at: millis(added_at),
                content_hash: test_content_hash(),
                format: BookFormat::Epub,
            },
        }
    }

    fn pull_response(mutations: Vec<ServerMutation>, next_cursor: Option<LibraryRevision>, has_more: bool) -> PullStateResponse {
        PullStateResponse { mutations, next_cursor, has_more }
    }

    fn binary_sync_response<T: serde::Serialize>(value: &T) -> axum::response::Response {
        use axum::response::IntoResponse;

        let encoded = sync_common::wire::encode_transport(value, true).unwrap();
        let mut response = encoded.bytes.into_response();
        response.headers_mut().insert(axum::http::header::CONTENT_TYPE, axum::http::HeaderValue::from_static(sync_common::wire::MEDIA_TYPE));
        if encoded.encoding == sync_common::wire::ContentEncoding::Zstd {
            response.headers_mut().insert(axum::http::header::CONTENT_ENCODING, axum::http::HeaderValue::from_static(sync_common::wire::CONTENT_ENCODING_ZSTD));
        }
        response
    }

    fn decode_sync_exchange(headers: &axum::http::HeaderMap, body: &[u8]) -> sync_common::SyncExchangeRequest {
        let encoding = match headers.get(axum::http::header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()) {
            Some(value) if value.eq_ignore_ascii_case(sync_common::wire::CONTENT_ENCODING_ZSTD) => sync_common::wire::ContentEncoding::Zstd,
            _ => sync_common::wire::ContentEncoding::Identity,
        };
        sync_common::wire::decode_transport(body, encoding, sync_common::wire::MAX_DECODED_REQUEST_BYTES).unwrap()
    }

    /// Domain validity is a property of the wire types, not of a batch-level
    /// check: a mutation carrying an invalid position or timestamp cannot be
    /// constructed, so it can never reach application. `validate_pull_batch`
    /// is left to police revision ordering alone (see the test below).
    #[test]
    fn invalid_domain_values_cannot_be_represented_on_the_wire() {
        assert!(sync_common::ReadingPosition::parse("not-a-cfi").is_err());
        assert!(sync_common::ReadingPosition::parse("epubcfi(/6/2)").is_ok());
        assert!(sync_common::ReadingPosition::parse("pdfpage(3)").is_ok());
        assert!(sync_common::UnixMillis::new(i64::MAX as u64 + 1).is_err());
    }

    #[test]
    fn pull_batch_requires_strict_sequences_after_the_cursor() {
        let valid = pull_response(vec![pulled_change(11, deleted(1)), pulled_change(12, deleted(2))], LibraryRevision::new(12).ok(), false);
        assert_eq!(validate_pull_batch(&valid, Some(LibraryRevision::new(10).unwrap())), Ok(()));

        let repeated = pull_response(vec![pulled_change(11, deleted(1)), pulled_change(11, deleted(2))], LibraryRevision::new(11).ok(), false);
        assert!(validate_pull_batch(&repeated, Some(LibraryRevision::new(10).unwrap())).is_err());
        assert!(validate_pull_batch(&valid, Some(LibraryRevision::new(11).unwrap())).is_err());
    }

    #[tokio::test]
    async fn production_client_drains_all_pull_pages_before_finishing() {
        #[derive(Clone)]
        struct PageState(Arc<std::sync::atomic::AtomicUsize>);

        async fn pull_page(axum::extract::State(state): axum::extract::State<PageState>, headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            state.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let request = decode_sync_exchange(&headers, &body);
            let pull = match request.cursor.map(LibraryRevision::get) {
                None => pull_response(vec![pulled_change(1, added(1, "Paged book", 10))], LibraryRevision::new(1).ok(), true),
                Some(1) => pull_response(vec![pulled_change(2, deleted(1))], LibraryRevision::new(2).ok(), false),
                _ => pull_response(Vec::new(), request.cursor, false),
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

        manager.pull_events_from_server(None).await.unwrap();

        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        let conn = manager.db_pool.get().unwrap();
        assert_eq!(db::get_sync_metadata(&conn, "last_pull_server_seq").unwrap().as_deref(), Some("2"));
        server.abort();
    }

    #[tokio::test]
    async fn production_client_persists_exact_final_cursor_beyond_last_mutation() {
        async fn pull_page(headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            let request = decode_sync_exchange(&headers, &body);
            binary_sync_response(&SyncExchangeResponse {
                push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() },
                pull: pull_response(vec![pulled_change(1, added(1, "Cursor book", 10))], request.cursor.or(LibraryRevision::new(10).ok()), false),
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

        manager.pull_events_from_server(None).await.unwrap();

        let conn = manager.db_pool.get().unwrap();
        assert_eq!(db::get_sync_metadata(&conn, "last_pull_server_seq").unwrap().as_deref(), Some("10"));
        server.abort();
    }

    #[tokio::test]
    async fn production_client_persists_advanced_cursor_from_empty_final_page() {
        async fn pull_page(headers: axum::http::HeaderMap, body: axum::body::Bytes) -> axum::response::Response {
            let request = decode_sync_exchange(&headers, &body);
            assert_eq!(request.cursor.map(LibraryRevision::get), Some(10));
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
        }

        manager.pull_events_from_server(LibraryRevision::new(10).ok()).await.unwrap();

        let conn = manager.db_pool.get().unwrap();
        assert_eq!(db::get_sync_metadata(&conn, "last_pull_server_seq").unwrap().as_deref(), Some("12"));
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
            match request.cursor.map(LibraryRevision::get) {
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
                    binary_sync_response(&SyncExchangeResponse {
                        push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() },
                        pull: pull_response(vec![pulled_change(2, deleted(1))], LibraryRevision::new(2).ok(), false),
                    })
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

        manager.exchange_events_with_server(None, None).await.unwrap();

        assert_eq!(state.exchanges.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(state.pulls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let conn = manager.db_pool.get().unwrap();
        assert_eq!(db::get_sync_metadata(&conn, "last_pull_server_seq").unwrap().as_deref(), Some("2"));
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
            binary_sync_response(&SyncExchangeResponse { push, pull: pull_response(Vec::new(), request.cursor, false) })
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
                    rusqlite::params![sequence as i64, mutation.mutation_id.to_string(), sequence.to_string(), sync_common::wire::encode(&mutation.body).unwrap(), mutation.changed_at.storage_i64()],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }

        assert!(manager.exchange_events_with_server(None, None).await.is_err());

        assert_eq!(state.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        let rejected = state.rejected.lock().unwrap().unwrap();
        let conn = manager.db_pool.get().unwrap();
        let pending = db::get_pending_mutations(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].mutation_id, rejected);
        server.abort();
    }

    fn reading_change(sequence: u64, book_id: u64, cfi: &str, changed_at: u64) -> StateMutation {
        StateMutation {
            mutation_id: sync_common::MutationId::new(),
            body: MutationBody::ReadingPosition { book_id: BookId::new(book_id), value: ReadingPositionState { location: sync_common::ReadingPosition::parse(cfi).unwrap(), progress: 0.0 } },
            changed_at: millis(changed_at),
            replica_seq: local_seq(sequence),
        }
    }

    #[test]
    fn push_pages_are_bounded_by_count_and_preserve_sequence_order() {
        let mut changes = VecDeque::from((1..=sync_common::MAX_PUSH_MUTATIONS as u64 + 7).map(|sequence| reading_change(sequence, sequence, "epubcfi(/6/2)", sequence)).collect::<Vec<_>>());

        let first = take_push_page(&mut changes).unwrap();
        let second = take_push_page(&mut changes).unwrap();

        assert_eq!(first.len(), sync_common::MAX_PUSH_MUTATIONS);
        assert_eq!(second.len(), 7);
        assert_eq!(first.first().unwrap().replica_seq.get(), 1);
        assert_eq!(second.first().unwrap().replica_seq.get(), sync_common::MAX_PUSH_MUTATIONS as u64 + 1);
    }

    #[test]
    fn push_pages_stop_before_the_encoded_byte_target() {
        let large_text = "x".repeat(64 * 1024);
        let changes = (1..=100_u64)
            .map(|sequence| StateMutation {
                mutation_id: sync_common::MutationId::new(),
                body: MutationBody::Annotation {
                    annotation_id: uuid::Uuid::new_v4().to_string(),
                    value: AnnotationState {
                        book_id: BookId::new(sequence),
                        anchor: AnnotationAnchor::EpubCfi { cfi: "epubcfi(/6/2)".to_owned() },
                        exact_text: large_text.clone(),
                        style: AnnotationStyle::Highlight,
                        color: "yellow".to_owned(),
                        note: large_text.clone(),
                        created_at: millis(1),
                        modified_at: millis(1),
                        deleted: false,
                    },
                },
                changed_at: millis(sequence),
                replica_seq: local_seq(sequence),
            })
            .collect::<Vec<_>>();
        let mut queued = VecDeque::from(changes);

        let page = take_push_page(&mut queued).unwrap();
        let encoded_bytes = page.iter().map(|mutation| sync_common::wire::encode(mutation).unwrap().len()).sum::<usize>();

        assert!(!page.is_empty());
        assert!(page.len() < 100);
        assert!(encoded_bytes <= sync_common::MAX_PUSH_MUTATION_BYTES);
        assert!(!queued.is_empty());
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
        let dir_id = DirId::parse("singleton-dir").unwrap();
        let parent_id = DirId::parse("__root__").unwrap();
        let changes = vec![
            StateMutation {
                mutation_id: sync_common::MutationId::new(),
                // The tombstone wins an exact wall-clock tie through conflict_rank,
                // even though the later local sequence below says present.
                body: MutationBody::DirectoryLifecycle { dir_id: dir_id.clone(), deleted: true },
                changed_at: millis(10),
                replica_seq: local_seq(1),
            },
            StateMutation { mutation_id: sync_common::MutationId::new(), body: added(2, "canonical lifecycle", 10), changed_at: millis(10), replica_seq: local_seq(3) },
            StateMutation { mutation_id: sync_common::MutationId::new(), body: MutationBody::DirectoryLifecycle { dir_id, deleted: false }, changed_at: millis(10), replica_seq: local_seq(4) },
            reading_change(6, 1, "epubcfi(/6/18)", 10),
            reading_change(7, 1, "epubcfi(/6/20)", 20),
        ];

        let coalesced = coalesce_mutations_for_push(changes);
        assert_eq!(coalesced.iter().map(|change| change.replica_seq.get()).collect::<Vec<_>>(), vec![1, 3, 7]);
        assert!(matches!(&coalesced[0].body, MutationBody::DirectoryLifecycle { deleted: true, .. }));
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
        let validated = validate_push_response(&changes, &response).unwrap();
        assert_eq!(validated.accepted, vec![changes[1].mutation_id]);
        assert_eq!(validated.transient_rejections, vec![changes[0].mutation_id]);
    }

    #[test]
    fn push_acceptance_uses_mutation_identity() {
        let requested = vec![reading_change(1, 1, "epubcfi(/6/24)", 1), reading_change(2, 2, "epubcfi(/6/26)", 2)];
        let response = PushMutationsResponse { accepted: requested.iter().rev().map(|mutation| mutation.mutation_id).collect(), rejected: Vec::new() };
        assert_eq!(validate_push_response(&requested, &response).unwrap().accepted, response.accepted);
    }

    #[test]
    fn permanent_rejection_is_reported_by_mutation_identity() {
        let requested = vec![reading_change(1, 1, "epubcfi(/6/24)", 1)];
        let response = PushMutationsResponse { accepted: Vec::new(), rejected: vec![MutationRejection { mutation_id: requested[0].mutation_id, reason: MutationRejectionReason::InvalidPayload }] };
        assert_eq!(validate_push_response(&requested, &response).unwrap().permanent_rejections, vec![(requested[0].mutation_id, MutationRejectionReason::InvalidPayload)]);
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
                rusqlite::params![requested.replica_seq.get() as i64, requested.mutation_id.to_string(), sync_common::wire::encode(&requested.body).unwrap(), requested.changed_at.storage_i64()],
            )
            .unwrap();
        }
        let response = PushMutationsResponse { accepted: Vec::new(), rejected: vec![MutationRejection { mutation_id: requested.mutation_id, reason: MutationRejectionReason::InvalidPayload }] };

        assert!(manager.apply_push_response(std::slice::from_ref(&requested), &response).is_err());

        let conn = manager.db_pool.get().unwrap();
        assert!(db::get_pending_mutations(&conn).unwrap().is_empty());
    }

    #[test]
    fn incomplete_or_contradictory_acknowledgments_are_rejected() {
        let requested = vec![reading_change(1, 1, "epubcfi(/6/24)", 1), reading_change(2, 2, "epubcfi(/6/26)", 2)];
        let missing = PushMutationsResponse { accepted: vec![requested[0].mutation_id], rejected: Vec::new() };
        assert!(matches!(validate_push_response(&requested, &missing), Err(PushResponseError::InvalidAcknowledgments(_))));

        let contradictory = PushMutationsResponse { accepted: vec![requested[0].mutation_id], rejected: vec![MutationRejection { mutation_id: requested[0].mutation_id, reason: MutationRejectionReason::TimestampTooFarFuture }] };
        assert!(matches!(validate_push_response(&requested[..1], &contradictory), Err(PushResponseError::InvalidAcknowledgments(_))));
    }

    fn test_manager(root: &Path) -> SyncManager {
        test_manager_with_client(root, "client")
    }

    fn test_manager_with_client(root: &Path, client_id: &str) -> SyncManager {
        let manager = SqliteConnectionManager::file(root.join(crate::APP_HIDDEN_LIBRARY_DB_PATH)).with_init(|conn| {
            conn.pragma_update(None, "foreign_keys", "ON")?;
            db::configure_sync_connection(conn)?;
            Ok(())
        });
        let pool = Pool::builder().max_size(2).build(manager).unwrap();
        {
            let conn = pool.get().unwrap();
            db::init_db(&conn).unwrap();
        }
        SyncManager {
            db_pool: pool,
            client_id: ClientId::parse(client_id).unwrap(),
            server_url: ServerUrl::parse("http://127.0.0.1:9").unwrap(),
            http_client: reqwest::Client::new(),
            library_id: LibraryId::parse("library").unwrap(),
            library_root: LibraryRoot::open(root).unwrap(),
            event_tx: None,
            session: Arc::new(std::sync::RwLock::new(None)),
            session_path: root.join(crate::APP_HIDDEN_SESSION_PATH),
            transfer_runtime: TransferRuntime::default(),
            transfer_queue: Arc::new(TransferQueue::default()),
            sync_gate: tokio::sync::Mutex::new(()),
        }
    }

    #[tokio::test]
    #[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
    async fn production_http_postgres_syncs_a_backend_outbox_to_a_second_replica() {
        let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL"))
            .expect("valid E2E server URL");
        let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
        let session = bokheim_sync_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in to E2E server");
        let library_id = LibraryId::parse("backend-e2e").unwrap();
        bokheim_sync_client::upsert_library(&session, library_id.clone(), "Backend E2E".to_owned()).await.expect("create E2E library");

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = server_url;
        manager.library_id = library_id.clone();
        crate::auth::save_session_to(&session, &manager.session_path).unwrap();
        *manager.session.write().unwrap() = Some(session.clone());

        let conn = manager.db_pool.get().unwrap();
        let transaction = conn.unchecked_transaction().unwrap();
        db::upsert_dir(&transaction, &DirId::parse("backend-e2e-dir").unwrap(), &DirId::parse("__root__").unwrap(), &FileName::parse("Backend E2E").unwrap()).unwrap();
        transaction.commit().unwrap();

        manager.sync_once().await.expect("publish Backend outbox through real server");

        let observed = bokheim_sync_client::exchange_changes(
            &reqwest::Client::new(),
            &session,
            library_id,
            ClientId::parse("backend-e2e-observer").unwrap(),
            Vec::new(),
            None,
        )
        .await
        .expect("pull through real server");
        assert!(observed.pull.mutations.iter().any(|change| change.mutation.kind == "directory_name"));
    }

    #[tokio::test]
    #[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
    async fn production_http_postgres_preserves_offline_outbox_and_reconciles_a_second_backend() {
        let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL"))
            .expect("valid E2E server URL");
        let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
        let session = bokheim_sync_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in to E2E server");
        let library_id = LibraryId::parse("backend-e2e-offline").unwrap();
        bokheim_sync_client::upsert_library(&session, library_id.clone(), "Backend offline E2E".to_owned()).await.expect("create E2E library");

        let writer_root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(writer_root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut writer = test_manager_with_client(writer_root.path(), "backend-e2e-offline-writer");
        writer.library_id = library_id.clone();
        let offline_session = crate::auth::Session::try_new(
            ServerUrl::parse("http://127.0.0.1:9").unwrap(),
            session.user_id().to_owned(),
            session.email().to_owned(),
            session.token().to_owned(),
        )
        .unwrap();
        crate::auth::save_session_to(&offline_session, &writer.session_path).unwrap();
        *writer.session.write().unwrap() = Some(offline_session);

        // The writer has no network route while it makes its local changes.
        // A failed exchange must leave the durable outbox intact for a later
        // reconnection, rather than losing either local register.
        let first_dir = DirId::parse("backend-e2e-offline-a").unwrap();
        let second_dir = DirId::parse("backend-e2e-offline-b").unwrap();
        {
            let conn = writer.db_pool.get().unwrap();
            let transaction = conn.unchecked_transaction().unwrap();
            db::upsert_dir(&transaction, &first_dir, &DirId::parse("__root__").unwrap(), &FileName::parse("Offline A").unwrap()).unwrap();
            db::upsert_dir(&transaction, &second_dir, &DirId::parse("__root__").unwrap(), &FileName::parse("Offline B").unwrap()).unwrap();
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
        let mut observer = test_manager_with_client(observer_root.path(), "backend-e2e-offline-observer");
        observer.server_url = server_url;
        observer.library_id = library_id;
        crate::auth::save_session_to(&session, &observer.session_path).unwrap();
        *observer.session.write().unwrap() = Some(session);
        observer.sync_once().await.expect("second backend pulls the writer's offline history");

        let conn = observer.db_pool.get().unwrap();
        for (id, expected_name) in [(&first_dir, "Offline A"), (&second_dir, "Offline B")] {
            let actual_name: String = conn.query_row("SELECT name FROM dir WHERE id = ?1", [id.as_str()], |row| row.get(0)).unwrap();
            assert_eq!(actual_name, expected_name);
        }
    }

    #[tokio::test]
    #[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
    async fn production_http_postgres_drains_all_sync_pages_for_a_fresh_backend() {
        let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL")).unwrap();
        let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
        let session = bokheim_sync_client::login(&server_url, "admin@bokheim.local", &password).await.unwrap();
        let library_id = LibraryId::parse("backend-e2e-pagination").unwrap();
        bokheim_sync_client::upsert_library(&session, library_id.clone(), "Backend pagination E2E".to_owned()).await.unwrap();

        // Reading registers avoid filesystem materialization, leaving this as
        // a focused protocol test: 1,001 states must cross the 1,000-item
        // pull boundary and be drained by a fresh persistent client.
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let changes = (1..=sync_common::MAX_PULL_CHANGES as u64 + 1)
            .map(|index| StateMutation {
                mutation_id: sync_common::MutationId::new(),
                body: MutationBody::ReadingPosition {
                    book_id: BookId::new(index),
                    value: ReadingPositionState { location: sync_common::ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 0.5 },
                },
                changed_at: sync_common::UnixMillis::new(now).unwrap(),
                replica_seq: ReplicaSeq::new(index).unwrap(),
            })
            .collect::<Vec<_>>();
        for page in changes.chunks(sync_common::MAX_PUSH_MUTATIONS) {
            let response = bokheim_sync_client::exchange_changes(
                &reqwest::Client::new(),
                &session,
                library_id.clone(),
                ClientId::parse("backend-e2e-pagination-writer").unwrap(),
                page.to_vec(),
                None,
            )
            .await
            .expect("publish a page of reading registers");
            assert_eq!(response.push.accepted.len(), page.len());
            assert!(response.push.rejected.is_empty());
        }

        let observer_root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(observer_root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut observer = test_manager_with_client(observer_root.path(), "backend-e2e-pagination-observer");
        observer.server_url = server_url;
        observer.library_id = library_id;
        crate::auth::save_session_to(&session, &observer.session_path).unwrap();
        *observer.session.write().unwrap() = Some(session);
        observer.sync_once().await.expect("drain every remote synchronization page");
        let conn = observer.db_pool.get().unwrap();
        let positions: i64 = conn.query_row("SELECT COUNT(*) FROM book WHERE read_pos IS NOT NULL", [], |row| row.get(0)).unwrap();
        assert_eq!(positions, sync_common::MAX_PULL_CHANGES as i64 + 1);
        assert!(db::get_sync_metadata(&conn, "last_pull_server_seq").unwrap().is_some(), "the final cursor is persisted with the final page");
    }

    #[tokio::test]
    #[ignore = "requires the PostgreSQL sync server started by run_sync_e2e.sh"]
    async fn production_http_postgres_serves_assets_and_notification_endpoints() {
        let server_url = ServerUrl::parse(&std::env::var("SYNC_E2E_SERVER_URL").expect("SYNC_E2E_SERVER_URL"))
            .expect("valid E2E server URL");
        let password = std::env::var("SYNC_E2E_ADMIN_PASSWORD").expect("SYNC_E2E_ADMIN_PASSWORD");
        let observer_session = bokheim_sync_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in observer");
        let publisher_session = bokheim_sync_client::login(&server_url, "admin@bokheim.local", &password).await.expect("log in publisher");
        assert_eq!(bokheim_sync_client::current_account(&observer_session).await.unwrap().email, observer_session.email());

        // A second credential must receive the publisher's change notification;
        // the publisher's own credential is intentionally filtered by the hub.
        let mut notifications = bokheim_sync_client::connect_notifications(&observer_session).await.expect("connect authenticated notifications");
        assert!(matches!(tokio::time::timeout(Duration::from_secs(2), notifications.next()).await.unwrap().unwrap(), Some(sync_common::api::notifications::NotificationMessage::Hello { .. })));

        let library_id = LibraryId::parse("backend-e2e-assets").unwrap();
        bokheim_sync_client::upsert_library(&publisher_session, library_id.clone(), "Backend asset E2E".to_owned()).await.expect("create E2E library");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), notifications.next()).await.unwrap().unwrap(),
            Some(sync_common::api::notifications::NotificationMessage::CatalogueChanged)
        );
        notifications.close().await;
        assert!(bokheim_sync_client::list_libraries(&observer_session).await.unwrap().iter().any(|library| library.library_id == library_id));

        let client = reqwest::Client::new();
        let book_bytes = test_epub(b"real server asset endpoint coverage");
        let book_hash = ContentHash::parse(&blake3::hash(&book_bytes).to_hex().to_string()).unwrap();
        let manifest = bokheim_sync_client::negotiate_blobs(
            &client,
            &publisher_session,
            &library_id,
            vec![BlobManifestEntry { content_hash: book_hash.clone(), size_bytes: book_bytes.len() as u64 }],
        )
        .await
        .expect("negotiate missing book blob");
        assert_eq!(manifest.upload, vec![book_hash.clone()]);
        bokheim_sync_client::upload_book_bytes(&client, &publisher_session, &library_id, &book_hash, book_bytes.clone()).await.expect("upload negotiated book blob");

        let book_url = publisher_session.server_url().endpoint(&format!("api/libraries/{}/blobs/{}", library_id.as_str(), book_hash.as_str()));
        let head = client.head(book_url.clone()).bearer_auth(publisher_session.token()).send().await.unwrap();
        assert_eq!(head.status(), reqwest::StatusCode::OK);
        assert_eq!(head.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &book_bytes.len().to_string());
        let range = client.get(book_url).bearer_auth(publisher_session.token()).header(reqwest::header::RANGE, "bytes=4-11").send().await.unwrap();
        assert_eq!(range.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert_eq!(range.bytes().await.unwrap().as_ref(), &book_bytes[4..12]);
        assert_eq!(bokheim_sync_client::download_book_bytes(&client, &observer_session, &library_id, &book_hash).await.unwrap(), book_bytes);
        let owned = bokheim_sync_client::negotiate_blobs(
            &client,
            &observer_session,
            &library_id,
            vec![BlobManifestEntry { content_hash: book_hash.clone(), size_bytes: book_bytes.len() as u64 }],
        )
        .await
        .expect("negotiate uploaded book blob");
        assert_eq!(owned.owned, vec![book_hash.clone()]);

        let thumbnail = vec![0xff, 0xd8, 0xff, 0xd9];
        let thumbnail_url = publisher_session.server_url().endpoint(&format!("api/libraries/{}/thumbnails/{}", library_id.as_str(), book_hash.as_str()));
        assert_eq!(client.put(thumbnail_url.clone()).bearer_auth(publisher_session.token()).body(thumbnail.clone()).send().await.unwrap().status(), reqwest::StatusCode::OK);
        let thumbnail_head = client.head(thumbnail_url.clone()).bearer_auth(observer_session.token()).send().await.unwrap();
        assert_eq!(thumbnail_head.status(), reqwest::StatusCode::OK);
        assert_eq!(thumbnail_head.headers().get(reqwest::header::CONTENT_LENGTH).unwrap(), &thumbnail.len().to_string());
        assert_eq!(bokheim_sync_client::download_thumbnail_bytes(&client, &observer_session, &library_id, &book_hash).await.unwrap(), thumbnail);

        // Asset endpoints must reject malformed, unauthorized, and quota
        // exceeding requests without disturbing an already-owned blob.
        assert_eq!(client.head(publisher_session.server_url().endpoint(&format!("api/libraries/{}/blobs/{}", library_id.as_str(), book_hash.as_str()))).send().await.unwrap().status(), reqwest::StatusCode::UNAUTHORIZED);
        let invalid_range = client.get(publisher_session.server_url().endpoint(&format!("api/libraries/{}/blobs/{}", library_id.as_str(), book_hash.as_str())))
            .bearer_auth(publisher_session.token())
            .header(reqwest::header::RANGE, format!("bytes={}-", book_bytes.len()))
            .send()
            .await
            .unwrap();
        assert_eq!(invalid_range.status(), reqwest::StatusCode::RANGE_NOT_SATISFIABLE);
        let wrong_hash = ContentHash::parse(&"f".repeat(64)).unwrap();
        let missing_length = client.put(publisher_session.server_url().endpoint(&format!("api/libraries/{}/blobs/{}", library_id.as_str(), wrong_hash.as_str())))
            .bearer_auth(publisher_session.token())
            .body(reqwest::Body::wrap_stream(futures_util::stream::iter(vec![Ok::<_, std::io::Error>(axum::body::Bytes::from(book_bytes.clone()))])))
            .send()
            .await
            .unwrap();
        assert_eq!(missing_length.status(), reqwest::StatusCode::LENGTH_REQUIRED);
        let wrong_hash_upload = client.put(publisher_session.server_url().endpoint(&format!("api/libraries/{}/blobs/{}", library_id.as_str(), wrong_hash.as_str())))
            .bearer_auth(publisher_session.token())
            .body(book_bytes.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(wrong_hash_upload.status(), reqwest::StatusCode::BAD_REQUEST);

        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL");
        let database = sqlx::PgPool::connect(&database_url).await.unwrap();
        sqlx::query("UPDATE user_storage_account SET quota_bytes = 0 WHERE user_id = $1").bind(publisher_session.user_id()).execute(&database).await.unwrap();
        let quota_hash = ContentHash::parse(&"e".repeat(64)).unwrap();
        let quota = bokheim_sync_client::negotiate_blobs(
            &client,
            &publisher_session,
            &library_id,
            vec![BlobManifestEntry { content_hash: quota_hash.clone(), size_bytes: 1 }],
        )
        .await
        .expect("quota rejection is a valid manifest response");
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

        assert!(error.downcast_ref::<bokheim_sync_client::AuthError>().is_some_and(bokheim_sync_client::AuthError::is_unauthorized));
        assert!(manager.is_signed_in());
        assert!(manager.session_path.exists());
        server.abort();
    }

    #[test]
    fn materialize_remote_dir_move_renames_folder_and_prevents_scan_revert() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        {
            let conn = manager.db_pool.get().unwrap();
            conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'Old')", []).unwrap();
        }
        std::fs::create_dir_all(temp.path().join("Old")).unwrap();
        std::fs::write(temp.path().join("Old").join(".biblos_uuid"), "d1").unwrap();

        {
            let conn = manager.db_pool.get().unwrap();
            conn.execute("UPDATE dir SET name='New' WHERE id='d1'", []).unwrap();
        }
        manager.materialize_remote_dir_upserts(&[DirUpsertMaterialization::Move { dir_id: DirId::parse("d1").unwrap(), old_path: RelativeDirPath::parse("Old").unwrap(), new_path: RelativeDirPath::parse("New").unwrap() }]);

        assert!(!temp.path().join("Old").exists());
        assert_eq!(std::fs::read_to_string(temp.path().join("New").join(".biblos_uuid")).unwrap(), "d1");

        let conn = manager.db_pool.get().unwrap();
        let before: i64 = conn.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0)).unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        db::upsert_dir(&tx, &DirId::parse("d1").unwrap(), &DirId::parse("__root__").unwrap(), &sync_common::FileName::parse("New").unwrap()).unwrap();
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

        manager.materialize_remote_dir_deletes(&[RelativeDirPath::parse("A").unwrap(), RelativeDirPath::parse("A/B").unwrap()]);

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

        manager.materialize_remote_dir_deletes(&[RelativeDirPath::parse("A").unwrap()]);

        assert!(dir.exists(), "non-empty dir should not be removed");
        assert!(dir.join(".biblos_uuid").exists(), "marker must survive so scanner keeps existing identity");
    }

    #[test]
    fn state_driven_download_job_keeps_book_id_hash_and_path_distinct() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let conn = manager.db_pool.get().unwrap();
        conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'Shelf')", []).unwrap();
        conn.execute("INSERT INTO book(id, added_at, content_hash, format) VALUES ('7', 1, ?1, 'epub')", [TEST_CONTENT_HASH]).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, is_downloaded) VALUES ('d1', '7', 'book.epub', 'fp', 0)", []).unwrap();

        let job = manager.download_job(&conn, &test_content_hash(), TransferOrigin::UserInitiated).unwrap().expect("live placement should produce a download job");
        match job {
            TransferJob::DownloadBlob { content_hash, placement, origin } => {
                assert_eq!(content_hash, test_content_hash());
                assert_eq!(placement.book_id, BookId::new(7));
                assert_eq!(placement.rel_path.as_str(), "/Shelf/7.epub");
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
            conn.execute("INSERT INTO book(id, added_at, format) VALUES ('7', 1, 'epub')", []).unwrap();
        }

        assert!(manager.request_download(BookId::new(7)).is_err());

        {
            let conn = manager.db_pool.get().unwrap();
            conn.execute("UPDATE book SET content_hash=?1 WHERE id='7'", [TEST_CONTENT_HASH]).unwrap();
            conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, is_downloaded) VALUES ('__root__', '7', 'book.epub', 'fp', 0)", []).unwrap();
        }
        manager.request_download(BookId::new(7)).unwrap();

        let conn = manager.db_pool.get().unwrap();
        assert_eq!(db::get_asset_requests(&conn, BlobKind::Book).unwrap(), vec![(test_content_hash(), TransferOrigin::UserInitiated)]);
    }
}
