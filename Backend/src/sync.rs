// `include_sql!` generates query methods with one argument per SQL parameter.
#![allow(clippy::too_many_arguments)]

use crate::db;
use crate::path_types::{LibraryRoot, RelativeDirPath};
use crate::scanner::LibraryEvent;
use crate::transfer::{TransferJob, TransferOrigin, TransferRuntime, TransferWorker};
use crate::{BlobKind, BookId, BookPlacement, ContentHash, RelativeBookPath, ServerUrl};
use crossbeam_channel::Sender;
use include_sqlite_sql::{impl_sql, include_sql};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use sync_common::api::assets::{BlobsExistsRequest, ExistsResponse, ThumbnailsExistsRequest};
use sync_common::{ClientId, DirId, LibraryId, PullChangesResponse, PushChangesRequest, PushChangesResponse};

include_sql!("src/database/library.sql");

mod protocol;

use protocol::{coalesce_singletons_for_push, validate_pull_batch, validate_push_response, PushResponseError};

const MAX_EXISTS_HASHES_PER_REQUEST: usize = 4096;
const AUTH_EXPIRY_SAFETY_WINDOW_SECONDS: i64 = 24 * 60 * 60;

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
    sync_gate: tokio::sync::Mutex<()>,
}

#[derive(Debug)]
enum DirUpsertMaterialization {
    Create { dir_id: DirId },
    Move { dir_id: DirId, old_path: RelativeDirPath, new_path: RelativeDirPath },
}

impl SyncManager {
    pub(crate) fn new(
        db_pool: DbPool, client_id: ClientId, server_url: ServerUrl, library_id: LibraryId, library_root: LibraryRoot, session_path: PathBuf, load_session: bool, expected_account_id: Option<String>, event_tx: Option<Sender<LibraryEvent>>,
        transfer_runtime: TransferRuntime,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let http_client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;

        // Identity is server-issued and lives only in the session. Load any
        // saved session, but only trust it if it matches the user this local
        // library is bound to (guards against a stale session.json from a
        // different account being paired with this DB).
        let session = if load_session {
            crate::auth::Session::load_from(&session_path)?.filter(|session| {
                if session.server_url() == &server_url && expected_account_id.as_deref().is_none_or(|account_id| account_id == session.user_id()) {
                    true
                } else {
                    log::warn!("Ignoring session for {} because it belongs to server {}", session.username(), session.server_url());
                    false
                }
            })
        } else {
            None
        };
        let session = {
            let conn = db_pool.get()?;
            match db::get_sync_metadata(&conn, "bound_user_id")? {
                Some(bound) => session.filter(|s| {
                    if s.user_id() == bound {
                        true
                    } else {
                        log::warn!("Ignoring session for user {} (library bound to {})", s.user_id(), bound);
                        false
                    }
                }),
                // Never synced: opening while already logged in binds this
                // library to that account.
                None => {
                    if let Some(s) = session.as_ref() {
                        db::set_sync_metadata(&conn, "bound_user_id", s.user_id())?;
                    }
                    session
                }
            }
        };

        Ok(Self { db_pool, client_id, server_url, http_client, library_id, library_root, event_tx, session: Arc::new(std::sync::RwLock::new(session)), session_path, transfer_runtime, sync_gate: tokio::sync::Mutex::new(()) })
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
        session.save_to(&self.session_path).map_err(|error| error.to_string())?;
        let mut guard = self.session.write().map_err(|_| "session lock is poisoned".to_string())?;
        *guard = Some(session);
        Ok(())
    }

    pub fn clear_session(&self) -> Result<(), String> {
        {
            let mut guard = self.session.write().map_err(|_| "session lock is poisoned".to_string())?;
            *guard = None;
        }
        crate::auth::Session::clear_from(&self.session_path).map_err(|error| error.to_string())
    }

    pub async fn suspend(&self) -> Result<(), String> {
        let _sync_guard = self.sync_gate.lock().await;
        {
            let mut guard = self.session.write().map_err(|_| "session lock is poisoned".to_string())?;
            *guard = None;
        }
        let conn = self.db_pool.get().map_err(|error| error.to_string())?;
        db::clear_transfer_jobs(&conn).map_err(|error| error.to_string())
    }

    fn token(&self) -> Option<String> {
        self.session.read().ok().and_then(|guard| guard.as_ref().map(|session| session.token().to_owned()))
    }

    pub async fn sync_once(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.sync_once_with_origin(TransferOrigin::Background).await
    }

    pub async fn sync_once_with_origin(&self, origin: TransferOrigin) -> Result<(), Box<dyn std::error::Error>> {
        let _sync_guard = self.sync_gate.lock().await;
        if !self.is_signed_in() {
            // Local-only mode: nothing to push, nothing to pull.
            return Ok(());
        }
        if self.current_session().is_some_and(|session| session.expires_within(AUTH_EXPIRY_SAFETY_WINDOW_SECONDS)) {
            self.clear_session().map_err(std::io::Error::other)?;
            log::warn!("Authentication session expired or is too close to expiry; sign in again before synchronizing");
            return Ok(());
        }

        // Get last pull server_seq from metadata
        let last_pull_server_seq = {
            let conn = self.db_pool.get()?;
            match db::get_sync_metadata(&conn, "last_pull_server_seq")? {
                Some(value) => Some(sync_common::ServerSeq::new(value.parse::<u64>()?)?),
                None => None,
            }
        };

        // Push local events to server
        self.push_local_events().await?;

        // Pull new events from server since last pull
        self.pull_events_from_server(last_pull_server_seq).await?;

        // Sync book assets (files + thumbnails)
        self.sync_assets(origin).await?;

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
            self.session_path.clone(),
            self.transfer_runtime.clone(),
        )
        .run_pending(4)
        .await
    }

    fn require_authenticated_response(&self, response: reqwest::Response) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.clear_session().map_err(std::io::Error::other)?;
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "authentication expired; sign in again").into());
        }
        Ok(response.error_for_status()?)
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
        let upload_thumb_hashes = db::get_locally_present_blob_hashes(&conn, BlobKind::Thumbnail)?;
        // Expand book-id requests only after placement metadata supplies a
        // content hash. The book request remains durable while unresolved.
        for (book_id, origin) in db::get_book_download_requests(&conn)? {
            let hashes = db::get_book_hashes(&conn, book_id)?;
            if hashes.is_empty() {
                continue;
            }
            for hash in hashes {
                db::add_asset_request(&conn, BlobKind::Book, &hash, origin)?;
            }
            db::complete_book_download_request(&conn, book_id)?;
        }
        let pending_requests = db::get_asset_requests(&conn, BlobKind::Book)?;
        drop(conn);

        let existing_blobs = self.check_existing_blobs(upload_blob_hashes.clone()).await?;
        for content_hash in upload_blob_hashes {
            if existing_blobs.contains(&content_hash) {
                let conn = self.db_pool.get()?;
                db::mark_asset_remote_present(&conn, BlobKind::Book, &content_hash)?;
                continue;
            }
            let conn = self.db_pool.get()?;
            db::mark_asset_remote_missing(&conn, BlobKind::Book, &content_hash)?;
            let Some(local_path) = db::get_blob_local_path(&conn, BlobKind::Book, &content_hash)? else {
                continue;
            };
            jobs.push(TransferJob::UploadBlob { content_hash, local_path, placement: None, origin: upload_origin });
        }
        let existing_thumbs = self.check_existing_thumbnails(upload_thumb_hashes.clone()).await?;
        for content_hash in upload_thumb_hashes {
            if existing_thumbs.contains(&content_hash) {
                let conn = self.db_pool.get()?;
                db::mark_asset_remote_present(&conn, BlobKind::Thumbnail, &content_hash)?;
                continue;
            }
            let conn = self.db_pool.get()?;
            db::mark_asset_remote_missing(&conn, BlobKind::Thumbnail, &content_hash)?;
            let Some(thumbnail_path) = db::get_blob_local_path(&conn, BlobKind::Thumbnail, &content_hash)? else {
                continue;
            };
            jobs.push(TransferJob::UploadThumbnail { content_hash, thumbnail_path, book_id: None, origin: upload_origin });
        }

        // State-driven requested downloads: local-missing + server-present.
        let existing_pending = self.check_existing_blobs(pending_requests.iter().map(|(hash, _)| hash.clone()).collect()).await?;
        for (content_hash, origin) in pending_requests {
            let conn = self.db_pool.get()?;
            if db::is_blob_local(&conn, BlobKind::Book, &content_hash)? {
                db::complete_asset_request(&conn, BlobKind::Book, &content_hash)?;
                continue;
            }
            if existing_pending.contains(&content_hash) {
                db::mark_asset_remote_present(&conn, BlobKind::Book, &content_hash)?;
                let Some(job) = self.download_job(&conn, &content_hash, origin)? else {
                    continue;
                };
                jobs.push(job);
            } else {
                db::mark_asset_remote_missing(&conn, BlobKind::Book, &content_hash)?;
            }
        }

        // Eager thumbnails from known hashes: local-missing + server-present.
        let known_hash_list: Vec<ContentHash> = known_hashes.into_iter().collect();
        let existing_thumb_known = self.check_existing_thumbnails(known_hash_list.clone()).await?;
        for content_hash in known_hash_list {
            let conn = self.db_pool.get()?;
            if db::is_blob_local(&conn, BlobKind::Thumbnail, &content_hash)? {
                continue;
            }
            if existing_thumb_known.contains(&content_hash) {
                let Some(book_path) = db::get_book_paths_by_content_hash(&conn, &content_hash)?.into_iter().next() else {
                    continue;
                };
                db::mark_asset_remote_present(&conn, BlobKind::Thumbnail, &content_hash)?;
                jobs.push(TransferJob::DownloadThumbnail { content_hash, book_id: book_path.book_id, origin: upload_origin });
            } else {
                db::mark_asset_remote_missing(&conn, BlobKind::Thumbnail, &content_hash)?;
            }
        }

        TransferWorker::new(
            self.http_client.clone(),
            self.server_url.clone(),
            self.library_id.clone(),
            self.library_root.clone(),
            self.db_pool.clone(),
            self.event_tx.clone(),
            Arc::clone(&self.session),
            self.session_path.clone(),
            self.transfer_runtime.clone(),
        )
        .run(jobs, 4)
        .await?;
        Ok(())
    }

    pub fn request_download(&self, book_id: BookId) -> Result<(), Box<dyn std::error::Error>> {
        self.request_downloads(std::slice::from_ref(&book_id))
    }

    pub fn request_downloads(&self, book_ids: &[BookId]) -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = self.db_pool.get()?;
        // Record the logical request first. A later sync expands it to exact
        // hashes once placement metadata exists, so no in-memory fallback can
        // be lost on process exit.
        let transaction = conn.transaction()?;
        for book_id in book_ids {
            db::add_book_download_request(&transaction, book_id.clone(), TransferOrigin::UserInitiated)?;
        }
        transaction.commit()?;
        drop(conn);
        if let Some(tx) = &self.event_tx {
            for book_id in book_ids {
                let _ = tx.send(LibraryEvent::DownloadStatusChanged { book_id: book_id.clone(), state: crate::DownloadState::Queued });
            }
        }
        Ok(())
    }

    fn resolve_book_path(&self, rel_path: &RelativeBookPath) -> String {
        self.library_root.resolve_book(rel_path).to_string_lossy().into_owned()
    }

    fn thumbnail_path_for_content_hash(&self, content_hash: &ContentHash) -> String {
        crate::transfer::thumbnail_path_from_content_hash(&self.library_root, content_hash).to_string_lossy().into_owned()
    }

    fn download_job(&self, conn: &rusqlite::Connection, content_hash: &ContentHash, origin: TransferOrigin) -> Result<Option<TransferJob>, rusqlite::Error> {
        let book_paths = db::get_book_paths_by_content_hash(conn, content_hash)?;
        let Some(chosen) = book_paths.iter().find(|path| !Path::new(&self.resolve_book_path(&path.rel_path)).exists()).or_else(|| book_paths.first()) else {
            return Ok(None);
        };
        let local_path = self.resolve_book_path(&chosen.rel_path);
        db::upsert_blob_local_ref(conn, BlobKind::Book, content_hash, &local_path, Path::new(&local_path).exists())?;
        Ok(Some(TransferJob::DownloadBlob { content_hash: content_hash.clone(), placement: BookPlacement::new(chosen.book_id, chosen.rel_path.clone()), origin }))
    }

    async fn check_existing_blobs(&self, hashes: Vec<ContentHash>) -> Result<HashSet<ContentHash>, Box<dyn std::error::Error>> {
        if hashes.is_empty() {
            return Ok(HashSet::new());
        }
        let token = self.token().ok_or("not signed in")?;
        let endpoint = self.server_url.endpoint(&format!("api/libraries/{}/blobs/exists", urlencoding::encode(self.library_id.as_str())));
        let mut present = HashSet::new();
        for chunk in hashes.chunks(MAX_EXISTS_HASHES_PER_REQUEST) {
            let response = self.http_client.post(endpoint.clone()).bearer_auth(&token).json(&BlobsExistsRequest { hashes: chunk.to_vec() }).send().await?;
            let response = self.require_authenticated_response(response)?;
            present.extend(response.json::<ExistsResponse>().await?.present);
        }
        Ok(present)
    }

    async fn check_existing_thumbnails(&self, content_hashes: Vec<ContentHash>) -> Result<HashSet<ContentHash>, Box<dyn std::error::Error>> {
        if content_hashes.is_empty() {
            return Ok(HashSet::new());
        }
        let token = self.token().ok_or("not signed in")?;
        let endpoint = self.server_url.endpoint(&format!("api/libraries/{}/thumbnails/exists", urlencoding::encode(self.library_id.as_str())));
        let mut present = HashSet::new();
        for chunk in content_hashes.chunks(MAX_EXISTS_HASHES_PER_REQUEST) {
            let response = self.http_client.post(endpoint.clone()).bearer_auth(&token).json(&ThumbnailsExistsRequest { content_hashes: chunk.to_vec() }).send().await?;
            let response = self.require_authenticated_response(response)?;
            present.extend(response.json::<ExistsResponse>().await?.present);
        }
        Ok(present)
    }

    async fn push_local_events(&self) -> Result<(), Box<dyn std::error::Error>> {
        let changes = {
            let conn = self.db_pool.get()?;
            coalesce_singletons_for_push(db::get_unpushed_events(&conn)?)
        };

        if changes.is_empty() {
            return Ok(());
        }

        let requested_local_seqs: Vec<_> = changes.iter().map(|change| change.local_seq).collect();
        let session = self.current_session().ok_or("not signed in")?;
        let request = PushChangesRequest { user_id: session.user_id().to_owned(), library_id: self.library_id.clone(), client_id: self.client_id.clone(), changes };

        let response = self.http_client.post(self.server_url.endpoint("api/sync/push")).bearer_auth(session.token()).json(&request).send().await?;
        let response = self.require_authenticated_response(response)?;
        let push_response: PushChangesResponse = response.json().await?;
        let validated = validate_push_response(&requested_local_seqs, &push_response)?;

        for rejection in &push_response.rejected {
            if rejection.reason.is_transient() {
                log::warn!("Server deferred local sync event {}: {:?}", rejection.local_seq, rejection.reason);
            } else {
                log::error!("Server rejected local sync event {}: {:?}", rejection.local_seq, rejection.reason);
            }
        }

        if let Some(up_to_local_seq) = validated.checkpoint {
            let conn = self.db_pool.get()?;
            db::mark_events_pushed(&conn, up_to_local_seq.get())?;
        }
        if let Some((local_seq, reason)) = validated.permanent_rejection {
            return Err(PushResponseError::PermanentRejection { local_seq, reason }.into());
        }
        Ok(())
    }

    async fn pull_events_from_server(&self, since_server_seq: Option<sync_common::ServerSeq>) -> Result<(), Box<dyn std::error::Error>> {
        let session = self.current_session().ok_or("not signed in")?;
        let query = sync_common::PullQuery { user_id: session.user_id().to_owned(), library_id: self.library_id.clone(), client_id: self.client_id.clone(), since_server_seq };
        let response = self.http_client.get(self.server_url.endpoint("api/sync/pull")).query(&query).bearer_auth(session.token()).send().await?;
        let response = self.require_authenticated_response(response)?;
        let pull_response: PullChangesResponse = response.json().await?;
        let changes = pull_response.changes;
        validate_pull_batch(&changes, since_server_seq)?;

        if changes.is_empty() {
            self.drain_pending_fs_operations();
            return Ok(());
        }

        let mut conn = self.db_pool.get()?;
        let tx = conn.transaction()?;
        // Pull pages must be applied in server_seq order and the cursor must
        // advance only after commit. The server guarantees each page is
        // dependency-closed relative to the prior cursor, but not ordered within
        // the page: a placement or child dir can precede its parent dir. Defer FK
        // checks to commit so valid out-of-order pages apply atomically. If the
        // server stream has a real closure hole (for example an unparseable row
        // was skipped), commit fails and the cursor remains unchanged for retry.
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

        if let Some(last) = changes.last() {
            tx.set_sync_metadata("last_pull_server_seq", &last.server_seq.get().to_string())?;
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
        // Clear completed outbox rows. If the process dies before this point,
        // the scanner/startup path drains the same durable rows before scanning.
        self.drain_pending_fs_operations();
        Ok(())
    }

    fn drain_pending_fs_operations(&self) {
        match self.db_pool.get() {
            Ok(conn) => {
                if let Err(error) = db::drain_pending_fs_operations(&conn, self.library_root.as_path()) {
                    log::warn!("Failed to drain pending filesystem operations: {error}");
                }
            }
            Err(error) => log::warn!("Failed to get DB connection for pending filesystem operations: {error}"),
        }
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
            let Some((rel_path, content_hash)) = db::resolve_removed_book_dir(&conn, placement.dir_id.as_str(), placement.book_id, placement.file_name.as_str())? else {
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

            // Genuine removal: delete the local file.
            match std::fs::remove_file(&abs_old) {
                Ok(()) => log::info!("Deleted {abs_old} (removed on another device)"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => log::warn!("Failed to delete {abs_old} after remote removal: {e}"),
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
    use super::protocol::ValidatedPushResponse;
    use super::*;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process::{Child, Command};
    use sync_common::{ActorId, DirectoryState, FileName, Hlc, LocalChange, LocalSeq, PushRejection, PushRejectionReason, ServerChange, ServerSeq, SyncOp};

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

    fn local_seq(value: u64) -> LocalSeq {
        LocalSeq::new(value).unwrap()
    }

    fn pulled_change(server_sequence: u64, op: SyncOp) -> ServerChange {
        ServerChange {
            change: LocalChange { event_id: sync_common::EventId::new(), op, changed_at: 10, hlc: Hlc { pt: 10, l: 0 }, local_seq: local_seq(server_sequence) },
            actor_id: ActorId::parse("remote-device").unwrap(),
            server_seq: ServerSeq::new(server_sequence).unwrap(),
        }
    }

    #[test]
    fn pull_batch_requires_strict_sequences_after_the_cursor() {
        let valid = vec![pulled_change(11, SyncOp::BookDeleted { book_id: BookId::new(1) }), pulled_change(12, SyncOp::BookDeleted { book_id: BookId::new(2) })];
        assert_eq!(validate_pull_batch(&valid, Some(ServerSeq::new(10).unwrap())), Ok(()));

        let repeated = vec![pulled_change(11, SyncOp::BookDeleted { book_id: BookId::new(1) }), pulled_change(11, SyncOp::BookDeleted { book_id: BookId::new(2) })];
        assert!(validate_pull_batch(&repeated, Some(ServerSeq::new(10).unwrap())).is_err());
        assert!(validate_pull_batch(&valid, Some(ServerSeq::new(11).unwrap())).is_err());
    }

    #[test]
    fn pull_batch_rejects_invalid_domains_before_application() {
        let invalid_cfi = vec![pulled_change(1, SyncOp::ReadingPositionUpdated { book_id: BookId::new(1), cfi: "not-a-cfi".to_string() })];
        assert!(validate_pull_batch(&invalid_cfi, None).is_err());

        let mut excessive_hlc = pulled_change(1, SyncOp::BookDeleted { book_id: BookId::new(1) });
        excessive_hlc.change.hlc.pt = i64::MAX as u64 + 1;
        assert!(validate_pull_batch(&[excessive_hlc], None).is_err());

        let invalid_title = vec![pulled_change(1, SyncOp::BookAdded { book_id: BookId::new(1), title: "bad\ntitle".to_string(), added_at: 10 })];
        assert!(validate_pull_batch(&invalid_title, None).is_err());
    }

    fn reading_change(sequence: u64, book_id: u64, cfi: &str, hlc_pt: u64, hlc_l: u64) -> LocalChange {
        LocalChange { event_id: sync_common::EventId::new(), op: SyncOp::ReadingPositionUpdated { book_id: BookId::new(book_id), cfi: cfi.to_string() }, changed_at: hlc_pt, hlc: Hlc { pt: hlc_pt, l: hlc_l }, local_seq: local_seq(sequence) }
    }

    #[test]
    fn push_coalesces_reading_positions_per_book_by_winning_version() {
        let changes = vec![
            reading_change(1, 1, "old-one", 10, 0),
            LocalChange { event_id: sync_common::EventId::new(), op: SyncOp::BookAdded { book_id: BookId::new(3), title: "preserved".to_string(), added_at: 1 }, changed_at: 1, hlc: Hlc { pt: 1, l: 0 }, local_seq: local_seq(2) },
            reading_change(3, 2, "winner-two", 30, 0),
            reading_change(4, 1, "winner-one", 20, 0),
            // Later local sequence, but an older HLC: the server would retain
            // sequence 3, so client compaction must make the same choice.
            reading_change(5, 2, "loser-two", 20, 0),
        ];

        let coalesced = coalesce_singletons_for_push(changes);
        assert_eq!(coalesced.iter().map(|change| change.local_seq.get()).collect::<Vec<_>>(), vec![2, 3, 4]);
        assert!(matches!(&coalesced[1].op, SyncOp::ReadingPositionUpdated { cfi, .. } if cfi == "winner-two"));
        assert!(matches!(&coalesced[2].op, SyncOp::ReadingPositionUpdated { cfi, .. } if cfi == "winner-one"));
    }

    #[test]
    fn push_coalesces_every_protocol_singleton_but_preserves_append_events() {
        let dir_id = DirId::parse("singleton-dir").unwrap();
        let parent_id = DirId::parse("__root__").unwrap();
        let changes = vec![
            LocalChange {
                event_id: sync_common::EventId::new(),
                // The tombstone wins an exact-HLC tie through conflict_rank,
                // even though the later local sequence below says present.
                op: SyncOp::DirState { dir_id: dir_id.clone(), directory: DirectoryState::Deleted { parent_id: parent_id.clone(), name: FileName::parse("Deleted").unwrap() } },
                changed_at: 10,
                hlc: Hlc { pt: 10, l: 0 },
                local_seq: local_seq(1),
            },
            LocalChange { event_id: sync_common::EventId::new(), op: SyncOp::BookOpened { book_id: BookId::new(1), last_read: 10 }, changed_at: 10, hlc: Hlc { pt: 10, l: 0 }, local_seq: local_seq(2) },
            LocalChange { event_id: sync_common::EventId::new(), op: SyncOp::BookAdded { book_id: BookId::new(2), title: "append event".to_string(), added_at: 10 }, changed_at: 10, hlc: Hlc { pt: 10, l: 0 }, local_seq: local_seq(3) },
            LocalChange {
                event_id: sync_common::EventId::new(),
                op: SyncOp::DirState { dir_id, directory: DirectoryState::Present { parent_id, name: FileName::parse("Present").unwrap() } },
                changed_at: 10,
                hlc: Hlc { pt: 10, l: 0 },
                local_seq: local_seq(4),
            },
            LocalChange { event_id: sync_common::EventId::new(), op: SyncOp::BookOpened { book_id: BookId::new(1), last_read: 20 }, changed_at: 20, hlc: Hlc { pt: 20, l: 0 }, local_seq: local_seq(5) },
            reading_change(6, 1, "old", 10, 0),
            reading_change(7, 1, "new", 20, 0),
        ];

        let coalesced = coalesce_singletons_for_push(changes);
        assert_eq!(coalesced.iter().map(|change| change.local_seq.get()).collect::<Vec<_>>(), vec![1, 3, 5, 7]);
        assert!(matches!(&coalesced[0].op, SyncOp::DirState { directory: DirectoryState::Deleted { .. }, .. }));
        assert!(matches!(&coalesced[1].op, SyncOp::BookAdded { .. }), "append event must not be coalesced");
    }

    #[test]
    fn coalesced_push_does_not_checkpoint_across_an_interleaved_rejection() {
        let changes = coalesce_singletons_for_push(vec![
            reading_change(1, 1, "old", 10, 0),
            LocalChange { event_id: sync_common::EventId::new(), op: SyncOp::BookAdded { book_id: BookId::new(2), title: "barrier".to_string(), added_at: 1 }, changed_at: 1, hlc: Hlc { pt: 1, l: 0 }, local_seq: local_seq(2) },
            reading_change(3, 1, "winner", 20, 0),
        ]);
        let requested: Vec<_> = changes.iter().map(|change| change.local_seq).collect();
        assert_eq!(requested, vec![local_seq(2), local_seq(3)]);

        let response = PushChangesResponse { handled_local_seqs: vec![local_seq(3)], rejected: vec![PushRejection { local_seq: local_seq(2), reason: PushRejectionReason::HlcTooFarFuture }] };
        assert_eq!(validate_push_response(&requested, &response).unwrap().checkpoint, None);
    }

    #[test]
    fn push_checkpoint_advances_through_a_fully_handled_batch() {
        let response = PushChangesResponse { handled_local_seqs: vec![local_seq(1), local_seq(2), local_seq(3)], rejected: Vec::new() };
        assert_eq!(validate_push_response(&[local_seq(1), local_seq(2), local_seq(3)], &response).unwrap(), ValidatedPushResponse { checkpoint: Some(local_seq(3)), permanent_rejection: None });
    }

    #[test]
    fn every_rejection_is_a_checkpoint_barrier() {
        let response = PushChangesResponse { handled_local_seqs: vec![local_seq(1), local_seq(3)], rejected: vec![PushRejection { local_seq: local_seq(2), reason: PushRejectionReason::HlcTooFarFuture }] };
        assert_eq!(validate_push_response(&[local_seq(1), local_seq(2), local_seq(3)], &response).unwrap(), ValidatedPushResponse { checkpoint: Some(local_seq(1)), permanent_rejection: None });
    }

    #[test]
    fn permanent_rejection_is_reported_without_checkpointing_past_it() {
        let response = PushChangesResponse { handled_local_seqs: vec![local_seq(1), local_seq(3)], rejected: vec![PushRejection { local_seq: local_seq(2), reason: PushRejectionReason::ValueOutsideStorageRange }] };
        assert_eq!(
            validate_push_response(&[local_seq(1), local_seq(2), local_seq(3)], &response).unwrap(),
            ValidatedPushResponse { checkpoint: Some(local_seq(1)), permanent_rejection: Some((local_seq(2), PushRejectionReason::ValueOutsideStorageRange)) }
        );
    }

    #[test]
    fn incomplete_or_contradictory_acknowledgments_are_rejected() {
        let missing = PushChangesResponse { handled_local_seqs: vec![local_seq(1), local_seq(3)], rejected: Vec::new() };
        assert!(matches!(validate_push_response(&[local_seq(1), local_seq(2), local_seq(3)], &missing), Err(PushResponseError::InvalidAcknowledgments(_))));

        let contradictory = PushChangesResponse { handled_local_seqs: vec![local_seq(1)], rejected: vec![PushRejection { local_seq: local_seq(1), reason: PushRejectionReason::HlcTooFarFuture }] };
        assert!(matches!(validate_push_response(&[local_seq(1)], &contradictory), Err(PushResponseError::InvalidAcknowledgments(_))));
    }

    fn production_device(root: &Path, server_url: &str, library_id: &str, client_id: &str, session: &crate::auth::Session) -> Result<SyncManager, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(root.join(crate::APP_HIDDEN_DIR))?;
        let sqlite = SqliteConnectionManager::file(root.join(crate::APP_HIDDEN_LIBRARY_DB_PATH)).with_init(|conn| {
            conn.pragma_update(None, "foreign_keys", "ON")?;
            conn.pragma_update(None, "journal_mode", "WAL")?;
            conn.pragma_update(None, "busy_timeout", "10000")?;
            Ok(())
        });
        let pool = Pool::builder().max_size(4).build(sqlite)?;
        {
            let conn = pool.get()?;
            db::init_db(&conn)?;
            db::set_sync_metadata(&conn, "client_id", client_id)?;
            db::set_sync_metadata(&conn, "bound_user_id", session.user_id())?;
        }
        let manager = SyncManager::new(
            pool,
            ClientId::parse(client_id)?,
            ServerUrl::parse(server_url)?,
            LibraryId::parse(library_id)?,
            LibraryRoot::open(root)?,
            root.join(crate::APP_HIDDEN_SESSION_PATH),
            true,
            None,
            None,
            TransferRuntime::default(),
        )?;
        *manager.session.write().unwrap() = Some(session.clone());
        Ok(manager)
    }

    struct ExternalSyncServer {
        child: Option<Child>,
        url: String,
    }

    impl ExternalSyncServer {
        async fn start(binary: &Path, database_url: &str, working_dir: &Path, admin_password: &str) -> Result<Self, Box<dyn std::error::Error>> {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
            let address = listener.local_addr()?;
            drop(listener);

            let child = Command::new(binary).current_dir(working_dir).env("DATABASE_URL", database_url).env("SYNC_SERVER_ADDR", address.to_string()).env("BOKHEIM_ADMIN_PASSWORD", admin_password).spawn()?;
            let mut server = Self { child: Some(child), url: format!("http://{address}") };
            let health_url = format!("{}/api/health", server.url);
            let client = reqwest::Client::new();

            for _ in 0..200 {
                if client.get(&health_url).send().await.is_ok_and(|response| response.status().is_success()) {
                    return Ok(server);
                }
                if let Some(status) = server.child.as_mut().and_then(|child| child.try_wait().ok()).flatten() {
                    return Err(std::io::Error::other(format!("sync server exited during startup with {status}")).into());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }

            server.stop();
            Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "sync server did not become healthy").into())
        }

        fn stop(&mut self) {
            if let Some(mut child) = self.child.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    impl Drop for ExternalSyncServer {
        fn drop(&mut self) {
            self.stop();
        }
    }

    fn set_reading_position_at_hlc(manager: &SyncManager, book_id: BookId, cfi: &str, hlc_pt: i64) -> Result<(), Box<dyn std::error::Error>> {
        let conn = manager.db_pool.get()?;
        conn.execute("UPDATE book SET read_pos=?2 WHERE id=?1", rusqlite::params![book_id.canonical(), cfi])?;
        let local_seq: i64 = conn.query_row("SELECT MAX(local_seq) FROM event_log WHERE event_type='reading_position_updated' AND entity_id=?1", [book_id.canonical()], |row| row.get(0))?;
        conn.execute("UPDATE event_log SET hlc_pt=?2, hlc_l=0 WHERE local_seq=?1", rusqlite::params![local_seq, hlc_pt])?;
        Ok(())
    }

    async fn production_signup(server_url: &str, admin_password: &str, username: &str) -> Result<crate::auth::Session, Box<dyn std::error::Error>> {
        let invite_response: serde_json::Value = reqwest::Client::new()
            .post(format!("{server_url}/api/admin/invites"))
            .basic_auth("bokheim-admin", Some(admin_password))
            .json(&serde_json::json!({ "note": username, "expires_in_days": 1 }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let invite = invite_response.get("invite").and_then(serde_json::Value::as_str).ok_or_else(|| std::io::Error::other("invite response omitted invite"))?;
        Ok(crate::auth::signup_at(&ServerUrl::parse(server_url)?, invite, username, "correct-horse-battery-staple").await?)
    }

    fn add_local_test_book(
        manager: &SyncManager, root: &Path, dir_id: &DirId, dir_name: &sync_common::FileName, file_name: &sync_common::FileName, bytes: &[u8], local_hash: u64,
    ) -> Result<(BookId, ContentHash, PathBuf), Box<dyn std::error::Error>> {
        let content_hash = ContentHash::parse(blake3::hash(bytes).to_hex().as_str())?;
        let book_id = BookId::from_content_hash(&content_hash);
        let path = root.join(dir_name.as_str()).join(file_name.as_str());
        std::fs::create_dir_all(path.parent().expect("book path has a parent"))?;
        std::fs::write(&path, bytes)?;
        let root_id = DirId::parse("__root__")?;
        let mut conn = manager.db_pool.get()?;
        let tx = conn.transaction()?;
        db::upsert_dir(&tx, dir_id, &root_id, dir_name)?;
        db::insert_book(&tx, book_id)?;
        db::insert_local_book_dir(&tx, dir_id, book_id, file_name, local_hash, &content_hash, 1)?;
        tx.commit()?;
        Ok((book_id, content_hash, path))
    }

    fn set_local_reading_position(manager: &SyncManager, book_id: BookId, cfi: &str) -> Result<(), Box<dyn std::error::Error>> {
        let conn = manager.db_pool.get()?;
        db::update_reading_position(&conn, book_id, &crate::EpubCfi::parse(cfi)?)?;
        Ok(())
    }

    fn remove_local_placement(manager: &SyncManager, root: &Path, dir_name: &sync_common::FileName, file_name: &sync_common::FileName, book_id: BookId, dir_id: &DirId) -> Result<(), Box<dyn std::error::Error>> {
        let path = root.join(dir_name.as_str()).join(file_name.as_str());
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let conn = manager.db_pool.get()?;
        conn.execute(
            "UPDATE book_dir SET deleted_at=CAST(strftime('%s','now') AS INTEGER) WHERE dir_id=?1 AND book_id=?2 AND file_name=?3 AND deleted_at IS NULL",
            rusqlite::params![dir_id.as_str(), book_id.canonical(), file_name.as_str()],
        )?;
        Ok(())
    }

    fn sync_checkpoints(manager: &SyncManager) -> Result<(Option<String>, Option<String>), Box<dyn std::error::Error>> {
        let conn = manager.db_pool.get()?;
        Ok((db::get_sync_metadata(&conn, "last_pushed_local_seq")?, db::get_sync_metadata(&conn, "last_pull_server_seq")?))
    }

    fn unreachable_local_url() -> Result<String, Box<dyn std::error::Error>> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        drop(listener);
        Ok(format!("http://{address}"))
    }

    fn book_replica_state(manager: &SyncManager, book_id: BookId, dir_id: &DirId, file_name: &sync_common::FileName) -> Result<(Option<String>, bool, String), Box<dyn std::error::Error>> {
        let conn = manager.db_pool.get()?;
        let read_pos = conn.query_row("SELECT read_pos FROM book WHERE id=?1", [book_id.canonical()], |row| row.get(0))?;
        let (live, content_hash) =
            conn.query_row("SELECT deleted_at IS NULL, content_hash FROM book_dir WHERE dir_id=?1 AND book_id=?2 AND file_name=?3", rusqlite::params![dir_id.as_str(), book_id.canonical(), file_name.as_str()], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        Ok((read_pos, live, content_hash))
    }

    struct DeterministicRng(u64);

    impl DeterministicRng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            self.0
        }

        fn index(&mut self, upper: usize) -> usize {
            (self.next() as usize) % upper
        }
    }

    /// Full production-path synchronization test. The runner provisions a
    /// throwaway PostgreSQL cluster and launches the real server binary:
    /// `Backend/tests/run_sync_e2e.sh`.
    #[tokio::test]
    #[ignore = "requires the isolated PostgreSQL runner Backend/tests/run_sync_e2e.sh"]
    async fn production_http_postgres_two_device_sync() -> Result<(), Box<dyn std::error::Error>> {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("run through Backend/tests/run_sync_e2e.sh");
        let server_binary = PathBuf::from(std::env::var("SYNC_E2E_SERVER_BIN").expect("run through Backend/tests/run_sync_e2e.sh"));
        let admin_password = "sync-e2e-admin-password";
        let server_disk = tempfile::tempdir()?;
        let books_dir = server_disk.path().join("books");
        let mut server = ExternalSyncServer::start(&server_binary, &database_url, server_disk.path(), admin_password).await?;
        let server_url = server.url.clone();

        let invite_response: serde_json::Value = reqwest::Client::new()
            .post(format!("{server_url}/api/admin/invites"))
            .basic_auth("bokheim-admin", Some(admin_password))
            .json(&serde_json::json!({ "note": "production sync e2e", "expires_in_days": 1 }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let invite = invite_response.get("invite").and_then(serde_json::Value::as_str).ok_or_else(|| std::io::Error::other("invite response omitted invite"))?;
        let session = crate::auth::signup_at(&ServerUrl::parse(&server_url)?, invite, "sync-e2e-user", "correct-horse-battery-staple").await?;
        let library_id = "production-e2e-library";
        let device_a_disk = tempfile::tempdir()?;
        let device_b_disk = tempfile::tempdir()?;
        let mut device_a = production_device(device_a_disk.path(), &server_url, library_id, "z-device", &session)?;
        let mut device_b = production_device(device_b_disk.path(), &server_url, library_id, "a-device", &session)?;

        let bytes = test_epub(b"production synchronization integration payload");
        let content_hash = ContentHash::parse(blake3::hash(&bytes).to_hex().as_str())?;
        let book_id = BookId::from_content_hash(&content_hash);
        let dir_id = DirId::parse("shelf-dir")?;
        let root_id = DirId::parse("__root__")?;
        let shelf_name = sync_common::FileName::parse("Shelf")?;
        let file_name = sync_common::FileName::parse("book.epub")?;
        let device_a_book = device_a_disk.path().join("Shelf").join("book.epub");
        std::fs::create_dir_all(device_a_book.parent().unwrap())?;
        std::fs::write(&device_a_book, &bytes)?;
        let mut thumbnail = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 3).write_to(&mut thumbnail, image::ImageOutputFormat::Png)?;
        let thumbnail = thumbnail.into_inner();
        let device_a_thumbnail = crate::transfer::thumbnail_path_from_content_hash(&device_a.library_root, &content_hash);
        std::fs::create_dir_all(device_a_thumbnail.parent().unwrap())?;
        std::fs::write(&device_a_thumbnail, &thumbnail)?;
        {
            let mut conn = device_a.db_pool.get()?;
            let tx = conn.transaction()?;
            db::upsert_dir(&tx, &dir_id, &root_id, &shelf_name)?;
            db::insert_book(&tx, book_id)?;
            db::insert_local_book_dir(&tx, &dir_id, book_id, &file_name, 1, &content_hash, 1)?;
            tx.commit()?;
        }

        device_a.sync_once().await?;
        let server_blob = books_dir.join(&content_hash.as_str()[..2]).join(content_hash.as_str());
        assert_eq!(std::fs::read(&server_blob)?, bytes);

        // The production HTTP/PostgreSQL authorization boundary requires both
        // library ownership and a current live reference to the hash.
        let intruder = production_signup(&server_url, admin_password, "sync-e2e-intruder").await?;
        let asset_url = format!("{server_url}/api/libraries/{library_id}/blobs/{content_hash}");
        let forbidden = reqwest::Client::new().get(&asset_url).bearer_auth(intruder.token()).send().await?;
        assert_eq!(forbidden.status(), reqwest::StatusCode::FORBIDDEN);
        let unreferenced = "b".repeat(64);
        let forbidden = reqwest::Client::new().get(format!("{server_url}/api/libraries/{library_id}/blobs/{unreferenced}")).bearer_auth(session.token()).send().await?;
        assert_eq!(forbidden.status(), reqwest::StatusCode::FORBIDDEN);

        // The real transport serves a verifiable open-ended range used by the
        // client's durable partial-download resume path.
        let resumed = reqwest::Client::new().get(&asset_url).bearer_auth(session.token()).header(reqwest::header::RANGE, "bytes=5-").header(reqwest::header::IF_RANGE, format!("\"{content_hash}\"")).send().await?;
        assert_eq!(resumed.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert_eq!(resumed.headers().get(reqwest::header::ETAG).and_then(|value| value.to_str().ok()), Some(format!("\"{content_hash}\"").as_str()));
        assert_eq!(resumed.bytes().await?.as_ref(), &bytes[5..]);

        // Replay the same outbox after a simulated checkpoint-loss crash.
        {
            let conn = device_a.db_pool.get()?;
            db::set_sync_metadata(&conn, "last_pushed_local_seq", "0")?;
        }
        device_a.sync_once().await?;
        let probe_query = sync_common::PullQuery { user_id: session.user_id().to_owned(), library_id: LibraryId::parse(library_id)?, client_id: ClientId::parse("probe-device")?, since_server_seq: None };
        let probe: PullChangesResponse = reqwest::Client::new().get(format!("{server_url}/api/sync/pull")).query(&probe_query).bearer_auth(session.token()).send().await?.error_for_status()?.json().await?;
        assert_eq!(probe.changes.len(), 3, "checkpoint-loss retry must not duplicate server events");

        // Restart the real HTTP process boundary while retaining DB and blobs.
        server.stop();
        server = ExternalSyncServer::start(&server_binary, &database_url, server_disk.path(), admin_password).await?;
        let restarted_url = server.url.clone();
        device_a.server_url = ServerUrl::parse(&restarted_url)?;
        device_b.server_url = ServerUrl::parse(&restarted_url)?;

        device_b.sync_once().await?;
        let device_b_thumbnail = crate::transfer::thumbnail_path_from_content_hash(&device_b.library_root, &content_hash);
        assert_eq!(std::fs::read(&device_b_thumbnail)?, thumbnail, "content-addressed thumbnail did not traverse the production server");
        {
            let conn = device_b.db_pool.get()?;
            let placements: i64 =
                conn.query_row("SELECT COUNT(*) FROM book_dir WHERE book_id=?1 AND dir_id=?2 AND file_name=?3 AND deleted_at IS NULL", rusqlite::params![book_id.canonical(), dir_id.as_str(), file_name.as_str()], |row| row.get(0))?;
            assert_eq!(placements, 1);
        }
        device_b.request_download(book_id)?;
        device_b.sync_once().await?;
        let device_b_book = device_b_disk.path().join("Shelf").join("book.epub");
        assert_eq!(std::fs::read(&device_b_book)?, bytes);

        // Equal-HLC singleton conflict: actor z wins although actor a pushes first.
        set_reading_position_at_hlc(&device_a, book_id, "epubcfi(/6/4)", 100)?;
        set_reading_position_at_hlc(&device_b, book_id, "epubcfi(/6/2)", 100)?;
        device_b.sync_once().await?;
        device_a.sync_once().await?;
        device_b.sync_once().await?;
        for device in [&device_a, &device_b] {
            let conn = device.db_pool.get()?;
            let cfi: String = conn.query_row("SELECT read_pos FROM book WHERE id=?1", [book_id.canonical()], |row| row.get(0))?;
            assert_eq!(cfi, "epubcfi(/6/4)");
        }

        // An old but live blob survives production GC.
        let old = std::time::SystemTime::now() - Duration::from_secs(2 * 60 * 60);
        std::fs::File::open(&server_blob)?.set_times(std::fs::FileTimes::new().set_modified(old))?;
        reqwest::Client::new().post(format!("{restarted_url}/api/admin/gc")).send().await?.error_for_status()?;
        assert!(server_blob.exists());

        // Removal propagates to device A's filesystem and makes the blob dead.
        {
            let conn = device_b.db_pool.get()?;
            conn.execute("UPDATE book_dir SET deleted_at=1 WHERE book_id=?1 AND dir_id=?2 AND file_name=?3", rusqlite::params![book_id.canonical(), dir_id.as_str(), file_name.as_str()])?;
        }
        device_b.sync_once().await?;
        device_a.sync_once().await?;
        assert!(!device_a_book.exists());
        reqwest::Client::new().post(format!("{restarted_url}/api/admin/gc")).send().await?.error_for_status()?;
        assert!(!server_blob.exists());

        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires the isolated PostgreSQL runner Backend/tests/run_sync_e2e.sh"]
    async fn production_http_postgres_offline_reconnect_converges() -> Result<(), Box<dyn std::error::Error>> {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("run through Backend/tests/run_sync_e2e.sh");
        let server_binary = PathBuf::from(std::env::var("SYNC_E2E_SERVER_BIN").expect("run through Backend/tests/run_sync_e2e.sh"));
        let admin_password = "sync-e2e-admin-password";
        let server_disk = tempfile::tempdir()?;
        let mut server = ExternalSyncServer::start(&server_binary, &database_url, server_disk.path(), admin_password).await?;
        let server_url = server.url.clone();
        let session = production_signup(&server_url, admin_password, "sync-offline-user").await?;
        let library_id = "production-offline-library";
        let device_a_disk = tempfile::tempdir()?;
        let device_b_disk = tempfile::tempdir()?;
        let device_a = production_device(device_a_disk.path(), &server_url, library_id, "offline-device-a", &session)?;
        let mut device_b = production_device(device_b_disk.path(), &server_url, library_id, "offline-device-b", &session)?;
        let dir_id = DirId::parse("offline-shelf-dir")?;
        let dir_name = sync_common::FileName::parse("OfflineShelf")?;

        let baseline_name = sync_common::FileName::parse("baseline.epub")?;
        let baseline_bytes = test_epub(b"offline baseline payload");
        let (baseline_book, _, _) = add_local_test_book(&device_a, device_a_disk.path(), &dir_id, &dir_name, &baseline_name, &baseline_bytes, 1)?;
        device_a.sync_once().await?;
        device_b.sync_once().await?;

        // Device B diverges locally while its transport is unavailable.
        set_local_reading_position(&device_b, baseline_book, "epubcfi(/6/2)")?;
        let offline_name = sync_common::FileName::parse("created-offline.epub")?;
        let offline_bytes = test_epub(b"created while device B was offline");
        let (offline_book, _, _) = add_local_test_book(&device_b, device_b_disk.path(), &dir_id, &dir_name, &offline_name, &offline_bytes, 2)?;
        let checkpoints_before_failure = sync_checkpoints(&device_b)?;
        device_b.server_url = ServerUrl::parse(&unreachable_local_url()?)?;
        assert!(device_b.sync_once().await.is_err(), "offline synchronization unexpectedly succeeded");
        assert_eq!(sync_checkpoints(&device_b)?, checkpoints_before_failure, "failed synchronization advanced a durable checkpoint");

        // Device A independently changes the same singleton and creates a book.
        set_local_reading_position(&device_a, baseline_book, "epubcfi(/6/4)")?;
        let online_name = sync_common::FileName::parse("created-online.epub")?;
        let online_bytes = test_epub(b"created by device A while device B was offline");
        let (online_book, _, _) = add_local_test_book(&device_a, device_a_disk.path(), &dir_id, &dir_name, &online_name, &online_bytes, 3)?;
        device_a.sync_once().await?;

        // Reconnect B, then exchange changes in both directions until quiescent.
        device_b.server_url = ServerUrl::parse(&server_url)?;
        device_b.sync_once().await?;
        device_a.sync_once().await?;
        device_b.sync_once().await?;

        for device in [&device_a, &device_b] {
            let conn = device.db_pool.get()?;
            let live_placements: i64 = conn.query_row("SELECT COUNT(*) FROM book_dir WHERE dir_id=?1 AND deleted_at IS NULL", [dir_id.as_str()], |row| row.get(0))?;
            assert_eq!(live_placements, 3, "offline and online additions did not converge");
        }
        let a_position = book_replica_state(&device_a, baseline_book, &dir_id, &baseline_name)?.0;
        let b_position = book_replica_state(&device_b, baseline_book, &dir_id, &baseline_name)?.0;
        assert_eq!(a_position, b_position, "concurrent offline reading positions did not converge");

        for book_id in [baseline_book, offline_book, online_book] {
            device_a.request_download(book_id)?;
            device_b.request_download(book_id)?;
        }
        device_a.sync_once().await?;
        device_b.sync_once().await?;
        for (name, expected) in [(&baseline_name, baseline_bytes.as_slice()), (&offline_name, offline_bytes.as_slice()), (&online_name, online_bytes.as_slice())] {
            assert_eq!(std::fs::read(device_a_disk.path().join(dir_name.as_str()).join(name.as_str()))?, expected);
            assert_eq!(std::fs::read(device_b_disk.path().join(dir_name.as_str()).join(name.as_str()))?, expected);
        }

        server.stop();
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires the isolated PostgreSQL runner Backend/tests/run_sync_e2e.sh"]
    async fn production_http_postgres_repeated_partition_heal_stress() -> Result<(), Box<dyn std::error::Error>> {
        const CYCLES: usize = 12;
        const OFFLINE_UPDATES_PER_CYCLE: usize = 9;

        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("run through Backend/tests/run_sync_e2e.sh");
        let server_binary = PathBuf::from(std::env::var("SYNC_E2E_SERVER_BIN").expect("run through Backend/tests/run_sync_e2e.sh"));
        let admin_password = "sync-e2e-admin-password";
        let server_disk = tempfile::tempdir()?;
        let mut server = ExternalSyncServer::start(&server_binary, &database_url, server_disk.path(), admin_password).await?;
        let live_url = server.url.clone();
        let session = production_signup(&live_url, admin_password, "sync-partition-user").await?;
        let library_id = "production-partition-library";
        let device_disks = [tempfile::tempdir()?, tempfile::tempdir()?, tempfile::tempdir()?];
        let mut devices = vec![
            production_device(device_disks[0].path(), &live_url, library_id, "partition-device-a", &session)?,
            production_device(device_disks[1].path(), &live_url, library_id, "partition-device-b", &session)?,
            production_device(device_disks[2].path(), &live_url, library_id, "partition-device-c", &session)?,
        ];
        let device_count = devices.len();
        let dir_id = DirId::parse("partition-shelf-dir")?;
        let dir_name = sync_common::FileName::parse("PartitionShelf")?;
        let file_name = sync_common::FileName::parse("partition.epub")?;
        let bytes = test_epub(b"repeated partition and healing payload");
        let (book_id, _, _) = add_local_test_book(&devices[0], device_disks[0].path(), &dir_id, &dir_name, &file_name, &bytes, 1)?;
        devices[0].sync_once().await?;
        for device in &devices[1..] {
            device.sync_once().await?;
        }

        for cycle in 0..CYCLES {
            let isolated = cycle % device_count;
            let unreachable = ServerUrl::parse(&unreachable_local_url()?)?;
            devices[isolated].server_url = unreachable;

            for update in 0..OFFLINE_UPDATES_PER_CYCLE {
                let cfi = format!("epubcfi(/6/{})", cycle * 100 + update + 2);
                set_local_reading_position(&devices[isolated], book_id, &cfi)?;
            }
            let checkpoints_before_failure = sync_checkpoints(&devices[isolated])?;
            assert!(devices[isolated].sync_once().await.is_err(), "cycle {cycle}: isolated client unexpectedly synchronized");
            assert_eq!(sync_checkpoints(&devices[isolated])?, checkpoints_before_failure, "cycle {cycle}: failed synchronization advanced a checkpoint");

            for offset in 1..device_count {
                let connected = (isolated + offset) % device_count;
                let cfi = format!("epubcfi(/8/{})", cycle * 10 + offset);
                set_local_reading_position(&devices[connected], book_id, &cfi)?;
                devices[connected].sync_once().await?;
            }

            devices[isolated].server_url = ServerUrl::parse(&live_url)?;
            if cycle % 2 == 0 {
                // sync_once pushes before pulling. First let connected clients
                // publish their state, then reconnect the isolated client.
                for offset in 1..device_count {
                    let connected = (isolated + offset) % device_count;
                    devices[connected].sync_once().await?;
                }
                devices[isolated].sync_once().await?;
            } else {
                // Reverse who reconnects first on alternate cycles.
                devices[isolated].sync_once().await?;
                for offset in 1..device_count {
                    let connected = (isolated + offset) % device_count;
                    devices[connected].sync_once().await?;
                }
            }
            for _ in 0..2 {
                for device in &devices {
                    device.sync_once().await?;
                }
            }

            let expected = book_replica_state(&devices[0], book_id, &dir_id, &file_name)?;
            for (index, device) in devices.iter().enumerate().skip(1) {
                assert_eq!(book_replica_state(device, book_id, &dir_id, &file_name)?, expected, "cycle {cycle}: replica {index} diverged after healing");
            }
        }

        let probe_query = sync_common::PullQuery { user_id: session.user_id().to_owned(), library_id: LibraryId::parse(library_id)?, client_id: ClientId::parse("partition-probe")?, since_server_seq: None };
        let probe: PullChangesResponse = reqwest::Client::new().get(format!("{live_url}/api/sync/pull")).query(&probe_query).bearer_auth(session.token()).send().await?.error_for_status()?.json().await?;
        assert_eq!(probe.changes.iter().filter(|change| change.op.event_type() == "reading_position_updated").count(), 1, "all offline reading histories must collapse to one server register");

        server.stop();
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires the isolated PostgreSQL runner Backend/tests/run_sync_e2e.sh"]
    async fn production_http_postgres_seeded_multi_replica_stress() -> Result<(), Box<dyn std::error::Error>> {
        const SEED: u64 = 0x5eed_cafe_f00d_2026;
        const STEPS: usize = 64;

        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("run through Backend/tests/run_sync_e2e.sh");
        let server_binary = PathBuf::from(std::env::var("SYNC_E2E_SERVER_BIN").expect("run through Backend/tests/run_sync_e2e.sh"));
        let admin_password = "sync-e2e-admin-password";
        let server_disk = tempfile::tempdir()?;
        let mut server = ExternalSyncServer::start(&server_binary, &database_url, server_disk.path(), admin_password).await?;
        let session = production_signup(&server.url, admin_password, "sync-stress-user").await?;
        let library_id = "production-stress-library";
        let device_disks = [tempfile::tempdir()?, tempfile::tempdir()?, tempfile::tempdir()?];
        let mut devices = vec![
            production_device(device_disks[0].path(), &server.url, library_id, "stress-device-a", &session)?,
            production_device(device_disks[1].path(), &server.url, library_id, "stress-device-b", &session)?,
            production_device(device_disks[2].path(), &server.url, library_id, "stress-device-c", &session)?,
        ];
        let dir_id = DirId::parse("stress-shelf-dir")?;
        let dir_name = sync_common::FileName::parse("StressShelf")?;
        let file_name = sync_common::FileName::parse("stress.epub")?;
        let bytes = test_epub(b"deterministic multi-replica stress payload");
        let (book_id, content_hash, _) = add_local_test_book(&devices[0], device_disks[0].path(), &dir_id, &dir_name, &file_name, &bytes, 1)?;
        devices[0].sync_once().await?;
        for device in &mut devices[1..] {
            device.sync_once().await?;
            device.request_download(book_id)?;
            device.sync_once().await?;
        }

        let mut rng = DeterministicRng(SEED);
        let mut operations: Vec<usize> = (0..STEPS).map(|step| step % 10).collect();
        for index in (1..operations.len()).rev() {
            let swap_with = rng.index(index + 1);
            operations.swap(index, swap_with);
        }
        let mut trace = Vec::with_capacity(STEPS);
        let mut operation_counts = [0_usize; 10];
        for (step, operation) in operations.into_iter().enumerate() {
            let actor = rng.index(devices.len());
            operation_counts[operation] += 1;
            let description = match operation {
                0..=2 => {
                    let cfi = format!("epubcfi(/6/{})", 2 + step * 2 + actor);
                    set_local_reading_position(&devices[actor], book_id, &cfi)?;
                    format!("device={actor} reading_position={cfi}")
                }
                3 => {
                    remove_local_placement(&devices[actor], device_disks[actor].path(), &dir_name, &file_name, book_id, &dir_id)?;
                    format!("device={actor} remove_placement")
                }
                4 => {
                    let (_, readded_hash, _) = add_local_test_book(&devices[actor], device_disks[actor].path(), &dir_id, &dir_name, &file_name, &bytes, (step + 10) as u64)?;
                    assert_eq!(readded_hash, content_hash);
                    format!("device={actor} readd_placement")
                }
                5 => {
                    devices[actor].sync_once().await?;
                    format!("device={actor} sync")
                }
                6 => {
                    let other = (actor + 1 + rng.index(devices.len() - 1)) % devices.len();
                    devices[actor].sync_once().await?;
                    devices[other].sync_once().await?;
                    format!("devices={actor},{other} sync")
                }
                7 => {
                    let cfi = format!("epubcfi(/8/{})", step + actor);
                    set_local_reading_position(&devices[actor], book_id, &cfi)?;
                    let checkpoints = sync_checkpoints(&devices[actor])?;
                    let live_url = devices[actor].server_url.clone();
                    devices[actor].server_url = ServerUrl::parse(&unreachable_local_url()?)?;
                    let failed = devices[actor].sync_once().await;
                    devices[actor].server_url = live_url;
                    assert!(failed.is_err(), "seed={SEED:#x} step={step}: offline synchronization unexpectedly succeeded");
                    assert_eq!(sync_checkpoints(&devices[actor])?, checkpoints, "seed={SEED:#x} step={step}: offline failure advanced a checkpoint");
                    format!("device={actor} local_change_then_offline_sync")
                }
                8 => {
                    let conn = devices[actor].db_pool.get()?;
                    db::set_sync_metadata(&conn, "last_pushed_local_seq", "0")?;
                    drop(conn);
                    devices[actor].sync_once().await?;
                    format!("device={actor} checkpoint_rewind_and_retry")
                }
                9 => {
                    server.stop();
                    server = ExternalSyncServer::start(&server_binary, &database_url, server_disk.path(), admin_password).await?;
                    for device in &mut devices {
                        device.server_url = ServerUrl::parse(&server.url)?;
                    }
                    format!("server_restart new_url={}", server.url)
                }
                _ => unreachable!(),
            };
            trace.push(description);
            eprintln!("stress seed={SEED:#x} step={step}: {}", trace.last().expect("trace entry"));
        }
        assert!(operation_counts.iter().all(|count| *count > 0), "seed={SEED:#x} did not cover every operation class: {operation_counts:?}");

        // Multiple rounds cover every order in which a device can push changes
        // after an earlier device has already pulled during the same round.
        for _ in 0..4 {
            for device in &mut devices {
                device.sync_once().await?;
            }
        }

        let expected = book_replica_state(&devices[0], book_id, &dir_id, &file_name)?;
        for (index, device) in devices.iter().enumerate().skip(1) {
            assert_eq!(book_replica_state(device, book_id, &dir_id, &file_name)?, expected, "seed={SEED:#x}: replica {index} diverged; trace={trace:#?}");
        }

        if expected.1 {
            for device in &mut devices {
                device.request_download(book_id)?;
                device.sync_once().await?;
            }
            for (index, disk) in device_disks.iter().enumerate() {
                assert_eq!(std::fs::read(disk.path().join(dir_name.as_str()).join(file_name.as_str()))?, bytes, "seed={SEED:#x}: live replica {index} has wrong file bytes; trace={trace:#?}");
            }
        } else {
            for (index, disk) in device_disks.iter().enumerate() {
                assert!(!disk.path().join(dir_name.as_str()).join(file_name.as_str()).exists(), "seed={SEED:#x}: deleted replica {index} retained its file; trace={trace:#?}");
            }
        }

        server.stop();
        Ok(())
    }

    fn test_manager(root: &Path) -> SyncManager {
        let manager = SqliteConnectionManager::file(root.join(crate::APP_HIDDEN_LIBRARY_DB_PATH)).with_init(|conn| {
            conn.pragma_update(None, "foreign_keys", "ON")?;
            Ok(())
        });
        let pool = Pool::builder().max_size(2).build(manager).unwrap();
        {
            let conn = pool.get().unwrap();
            db::init_db(&conn).unwrap();
        }
        SyncManager {
            db_pool: pool,
            client_id: ClientId::parse("client").unwrap(),
            server_url: ServerUrl::parse("http://127.0.0.1:9").unwrap(),
            http_client: reqwest::Client::new(),
            library_id: LibraryId::parse("library").unwrap(),
            library_root: LibraryRoot::open(root).unwrap(),
            event_tx: None,
            session: Arc::new(std::sync::RwLock::new(None)),
            session_path: root.join(crate::APP_HIDDEN_SESSION_PATH),
            transfer_runtime: TransferRuntime::default(),
            sync_gate: tokio::sync::Mutex::new(()),
        }
    }

    #[tokio::test]
    async fn expiring_session_is_cleared_before_any_network_request() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let session = crate::auth::Session::try_new_with_expiry(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string(), Some(now + 60)).unwrap();
        session.save_to(&manager.session_path).unwrap();
        *manager.session.write().unwrap() = Some(session);

        manager.sync_once().await.unwrap();

        assert!(!manager.is_signed_in());
        assert!(!manager.session_path.exists());
    }

    #[tokio::test]
    async fn unauthorized_sync_response_invalidates_persisted_session() {
        let app = axum::Router::new().route("/api/sync/pull", axum::routing::get(|| async { axum::http::StatusCode::UNAUTHORIZED }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let mut manager = test_manager(temp.path());
        manager.server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        let session = crate::auth::Session::try_new(manager.server_url.clone(), "user-1".to_string(), "reader".to_string(), TEST_AUTH_TOKEN.to_string()).unwrap();
        session.save_to(&manager.session_path).unwrap();
        *manager.session.write().unwrap() = Some(session);

        let error = manager.sync_once().await.unwrap_err();

        assert!(error.to_string().contains("authentication expired"));
        assert!(!manager.is_signed_in());
        assert!(!manager.session_path.exists());
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
        let before: i64 = conn.query_row("SELECT COUNT(*) FROM event_log", [], |r| r.get(0)).unwrap();
        let tx = conn.unchecked_transaction().unwrap();
        db::upsert_dir(&tx, &DirId::parse("d1").unwrap(), &DirId::parse("__root__").unwrap(), &sync_common::FileName::parse("New").unwrap()).unwrap();
        tx.commit().unwrap();
        let after: i64 = conn.query_row("SELECT COUNT(*) FROM event_log", [], |r| r.get(0)).unwrap();
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
        conn.execute("INSERT INTO book(id, added_at) VALUES ('7', 1)", []).unwrap();
        conn.execute("INSERT INTO book_dir(dir_id, book_id, file_name, local_hash, content_hash, is_downloaded) VALUES ('d1', '7', 'book.epub', 'fp', ?1, 0)", [TEST_CONTENT_HASH]).unwrap();

        let job = manager.download_job(&conn, &test_content_hash(), TransferOrigin::UserInitiated).unwrap().expect("live placement should produce a download job");
        match job {
            TransferJob::DownloadBlob { content_hash, placement, origin } => {
                assert_eq!(content_hash, test_content_hash());
                assert_eq!(placement.book_id, BookId::new(7));
                assert_eq!(placement.rel_path.as_str(), "/Shelf/book.epub");
                assert_eq!(origin, TransferOrigin::UserInitiated);
            }
            _ => panic!("expected blob download"),
        }
    }

    #[test]
    fn download_request_never_treats_book_id_as_content_hash() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
        let manager = test_manager(temp.path());
        {
            let conn = manager.db_pool.get().unwrap();
            conn.execute("INSERT INTO book(id, added_at) VALUES ('7', 1)", []).unwrap();
        }

        manager.request_download(BookId::new(7)).unwrap();

        let conn = manager.db_pool.get().unwrap();
        assert!(db::get_asset_requests(&conn, BlobKind::Book).unwrap().is_empty());
        assert_eq!(db::get_book_download_requests(&conn).unwrap(), vec![(BookId::new(7), TransferOrigin::UserInitiated)]);
    }
}
