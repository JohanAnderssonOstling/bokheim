use super::error::SyncError;
use super::error::SyncResult;
use super::transfer::{TransferOrigin, TransferQueue, TransferWorker};
use account_client::ServerUrl;
use library_files::asset_store::AssetStore;
use library_replica::TransferJobKind;
use library_runtime::database::Database;
use library_runtime::events::{LibraryEvent, LibraryEventSender};
use library_runtime::interest::NotificationInterest;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use sync_common::ContentHash;
use sync_common::{LibraryId, ReplicaId};
use sync_engine::SyncOutcome;

#[cfg(not(target_arch = "wasm32"))]
#[path = "library_sync/native.rs"]
mod host;
#[cfg(target_arch = "wasm32")]
#[path = "library_sync/web.rs"]
mod host;

#[derive(Clone, Copy)]
enum SyncMode {
    Full,
    Outbox,
    Repair,
}
struct SyncExchange {
    outcome: SyncOutcome,
    inventory_checked: bool,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PersistedSyncWork {
    pub(crate) assets: bool,
}

pub(crate) struct LibrarySyncConfig {
    pub(crate) database: Database,
    pub(crate) assets: AssetStore,
    pub(crate) replica_id: ReplicaId,
    pub(crate) server_url: ServerUrl,
    pub(crate) library_id: LibraryId,
    pub(crate) event_tx: Option<LibraryEventSender>,
    pub(crate) transfer_history_path: std::path::PathBuf,
    pub(crate) credentials: library_runtime::account::SharedSyncCredentials,
    pub(crate) account: Option<library_runtime::account::LibraryAccount>,
    pub(crate) notification_interest: NotificationInterest,
}

#[derive(Debug)]
pub struct LibrarySync {
    asset_storage_enabled: Arc<AtomicBool>,
    pub(super) database: Database,
    pub(super) assets: AssetStore,
    replica_id: ReplicaId,
    server_url: ServerUrl,
    pub(super) http_client: reqwest::Client,
    pub(super) library_id: LibraryId,
    event_tx: Option<LibraryEventSender>,
    pub(super) credentials: library_runtime::account::SharedSyncCredentials,
    account: Option<library_runtime::account::LibraryAccount>,
    notification_interest: NotificationInterest,
    transfer_queue: Arc<TransferQueue>,
    sync_gate: tokio::sync::Mutex<()>,
    asset_gate: tokio::sync::Mutex<()>,
    pub(crate) registration_gate: tokio::sync::Mutex<()>,
    inventory_checked: AtomicBool,
    metadata_activity: AtomicUsize,
    asset_activity: AtomicUsize,
}

impl Drop for LibrarySync {
    fn drop(&mut self) {
        // A detached transfer worker may still own the queue. Persist completed
        // history before this library manager can be closed and reopened.
        if let Err(error) = self.transfer_queue.flush_history() {
            log::warn!("Could not flush transfer history while closing library {}: {error}", self.library_id);
        }
    }
}

/// Counts overlapping work and clears activity on completion, error or cancellation.
struct SyncActivity<'a> {
    count: &'a AtomicUsize,
    events: Option<&'a LibraryEventSender>,
}
impl<'a> SyncActivity<'a> {
    fn begin(count: &'a AtomicUsize, events: Option<&'a LibraryEventSender>) -> Self {
        count.fetch_add(1, Ordering::AcqRel);
        if let Some(events) = events {
            let _ = events.try_send(LibraryEvent::TransferStatusChanged);
        }
        Self { count, events }
    }
}
impl Drop for SyncActivity<'_> {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::AcqRel);
        if let Some(events) = self.events {
            let _ = events.try_send(LibraryEvent::TransferStatusChanged);
        }
    }
}

impl LibrarySync {
    pub(crate) fn metadata_sync_running(&self) -> bool {
        self.metadata_activity.load(Ordering::Acquire) > 0
    }
    pub(crate) fn preparing_transfers(&self) -> bool {
        self.asset_activity.load(Ordering::Acquire) > 0
    }

    pub(crate) fn new(config: LibrarySyncConfig) -> Self {
        if let Some(events) = config.event_tx.clone() {
            config.database.observe_remote_pages(move || {
                let _ = events.try_send(LibraryEvent::ContentsChanged);
            });
        }
        let http_client = reqwest::Client::new();
        let transfer_queue = Arc::new(super::transfer::queue_with_history(&config.database, config.transfer_history_path));
        Self {
            asset_storage_enabled: Arc::new(AtomicBool::new(true)),
            database: config.database,
            assets: config.assets,
            replica_id: config.replica_id,
            server_url: config.server_url,
            http_client,
            library_id: config.library_id,
            event_tx: config.event_tx,
            credentials: config.credentials,
            account: config.account,
            notification_interest: config.notification_interest,
            transfer_queue,
            sync_gate: tokio::sync::Mutex::new(()),
            asset_gate: tokio::sync::Mutex::new(()),
            registration_gate: tokio::sync::Mutex::new(()),
            inventory_checked: AtomicBool::new(false),
            metadata_activity: AtomicUsize::new(0),
            asset_activity: AtomicUsize::new(0),
        }
    }

    /// Reconstruct work from durable state. Scheduler messages only wake the
    /// worker; losing a notification cannot lose a mutation or asset request.
    #[cfg(test)]
    pub(crate) async fn pending_work(&self) -> SyncResult<Option<PersistedSyncWork>> {
        self.database
            .command(library_runtime::storage_queue::Priority::Background, |conn| {
                let state = client_library::sync_state::get_library_sync_details(conn)?.has_unsynced_changes;
                let assets = client_library::assets::has_pending_asset_work(conn)?;
                Ok((state || assets).then_some(PersistedSyncWork { assets }))
            })
            .await
            .map_err(Into::into)
    }

    pub fn is_signed_in(&self) -> bool {
        self.credentials.credentials().is_some()
    }

    pub fn bound_user_id(&self) -> Result<Option<String>, String> {
        let conn = self.database.get().map_err(|error| error.to_string())?;
        client_library::sync_state::get_sync_metadata(&conn, "bound_user_id").map_err(|error| error.to_string())
    }

    /// Binds this local library to a server-issued user. Switching
    /// accounts clears account-specific remote presence and pull-cursor state,
    /// while retaining the local library itself.
    pub fn bind_account(&self, server_url: &str, user_id: &str) -> Result<(), String> {
        if server_url != self.server_url.as_str() {
            return Err(format!("account belongs to {server_url}, but this library syncs with {}", self.server_url));
        }
        let conn = self.database.get().map_err(|e| e.to_string())?;
        match client_library::sync_state::get_sync_metadata(&conn, "bound_user_id").map_err(|error| error.to_string())? {
            Some(bound) if bound != user_id => {
                client_library::sync_state::rebind_sync_account(&conn, user_id).map_err(|error| error.to_string())?;
                self.inventory_checked.store(false, Ordering::Release);
            }
            Some(_) => {}
            None => client_library::sync_state::set_sync_metadata(&conn, "bound_user_id", user_id).map_err(|error| error.to_string())?,
        }
        Ok(())
    }

    /// This handle observes the app-global account state. Workers adopt the
    /// current identity at execution time, avoiding account broadcasts.
    pub(crate) fn bind_current_account(&self) -> Result<bool, String> {
        let Some(account) = &self.account else {
            return Ok(self.is_signed_in());
        };
        let Some(session) = account.current().map_err(|error| error.to_string())? else {
            return Ok(false);
        };
        self.bind_account(session.server_url().as_str(), session.user_id())?;
        Ok(true)
    }

    pub(crate) fn account(&self) -> Option<&library_runtime::account::LibraryAccount> {
        self.account.as_ref()
    }

    /// A library worker, rather than the application registry, reacts to
    /// account changes.  The account object remains device-global, but this
    /// subscription is deliberately consumed only by the library that owns
    /// the corresponding durable work.
    pub(crate) fn subscribe_account_changes(&self) -> Option<tokio::sync::watch::Receiver<u64>> {
        self.account.as_ref().map(|account| account.subscribe())
    }

    /// The remote counterpart belongs to this concrete library. Its durable
    /// name and registration marker are stored alongside the library data.
    pub(crate) async fn ensure_server_library(&self) -> Result<(), String> {
        let account = self.account().ok_or("library has no account capability")?;
        let session = account.require().map_err(|error| error.to_string())?;
        let identity = format!("{}|{}", session.server_url(), session.user_id());
        if client_library::sync_state::get_sync_metadata(&self.database.get().map_err(|error| error.to_string())?, "server_library_created").map_err(|error| error.to_string())?.as_deref() == Some(identity.as_str()) {
            return Ok(());
        }
        let name = client_library::sync_state::get_sync_metadata(&self.database.get().map_err(|error| error.to_string())?, "remote_library_name").map_err(|error| error.to_string())?.ok_or("library has no remote name")?;
        let id = self.library_id;
        account.create_library(id, name).await.map_err(|error| error.to_string())?;
        client_library::sync_state::set_sync_metadata(&self.database.get().map_err(|error| error.to_string())?, "server_library_created", &identity).map_err(|error| error.to_string())?;
        Ok(())
    }

    fn sync_identity(&self) -> SyncResult<(Option<String>, Option<sync_transport::SyncCredentials>)> {
        let user = client_library::sync_state::get_sync_metadata(&*self.database.get()?, "bound_user_id")?;
        // Capture identity and token together. Account adoption can precede
        // rebinding on another thread; never send that interval's old outbox
        // using the new session's credentials.
        let credentials = if let Some(account) = &self.account {
            let session = account.current().map_err(SyncError::failed)?.ok_or(SyncError::AuthenticationRequired)?;
            if user.as_deref() != Some(session.user_id()) || session.server_url() != &self.server_url {
                return Err(SyncError::failed("account changed before synchronization"));
            }
            Some(sync_transport::SyncCredentials::new(session.server_url().clone(), session.token()))
        } else {
            self.credentials.credentials()
        };
        Ok((user, credentials))
    }

    pub(crate) async fn repair_inventory(&self) -> SyncResult<()> {
        // Inventory repair must not block foreground metadata synchronization.
        if !self.is_signed_in() {
            return Ok(());
        }
        client_library::operations::begin_sync(&*self.database.get()?)?;
        let _activity = SyncActivity::begin(&self.metadata_activity, self.event_tx.as_ref());
        let (user, credentials) = self.sync_identity()?;
        let Some(exchange) = host::exchange(self, &user, credentials, SyncMode::Repair).await? else { return Ok(()) };
        let mut conn = self.database.get()?;
        let tx = client_library::schema::library_write_transaction(&mut conn)?;
        super::ensure_sync_account(&tx, &user)?;
        if exchange.inventory_checked {
            self.inventory_checked.store(true, Ordering::Release);
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) async fn synchronize_state(&self) -> SyncResult<SyncOutcome> {
        self.synchronize_state_mode(false).await
    }

    pub(crate) async fn drain_outbox(&self) -> SyncResult<SyncOutcome> {
        self.synchronize_state_mode(true).await
    }

    async fn synchronize_state_mode(&self, outbox_only: bool) -> SyncResult<SyncOutcome> {
        let _sync_guard = host::lock(self).await;
        if !self.is_signed_in() {
            return Ok(SyncOutcome::default());
        }
        let _activity = SyncActivity::begin(&self.metadata_activity, self.event_tx.as_ref());
        let _interest = self.notification_interest.work();
        let (user, credentials) = self.sync_identity()?;
        let mode = if outbox_only { SyncMode::Outbox } else { SyncMode::Full };
        let Some(exchange) = host::exchange(self, &user, credentials, mode).await? else { return Ok(SyncOutcome::default()) };
        // This checkpoint covers publishable state. Books awaiting upload keep
        // their mutations in the outbox; upload completion wakes another pass.
        let completed_at = u64::try_from(web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH)?.as_millis())?;
        let mut conn = self.database.get()?;
        let tx = client_library::schema::library_write_transaction(&mut conn)?;
        super::ensure_sync_account(&tx, &user)?;
        if !outbox_only {
            client_library::sync_state::set_sync_metadata(&tx, "last_successful_sync_at", &completed_at.to_string())?;
        }
        client_library::operations::finish_sync_in_transaction(&tx)?;
        if exchange.inventory_checked {
            self.inventory_checked.store(true, Ordering::Release);
        }
        tx.commit()?;
        Ok(exchange.outcome)
    }

    pub(crate) fn set_asset_storage_enabled(&self, enabled: bool) {
        self.asset_storage_enabled.store(enabled, Ordering::Release);
    }

    pub(crate) fn asset_storage_enabled(&self) -> bool {
        self.asset_storage_enabled.load(Ordering::Acquire)
    }

    pub(crate) fn prepare_asset_storage_sync(&self) -> SyncResult<()> {
        client_library::operations::begin_sync(&*self.database.get()?)?;
        Ok(())
    }

    pub(crate) async fn reconcile_assets(&self) -> SyncResult<()> {
        let _asset_guard = self.asset_gate.lock().await;
        if !self.is_signed_in() {
            return Ok(());
        }
        let _activity = SyncActivity::begin(&self.asset_activity, self.event_tx.as_ref());
        let result = self.sync_assets().await;
        result
    }

    #[cfg(test)]
    async fn sync_once(&self) -> SyncResult<()> {
        self.synchronize_state().await?;
        self.reconcile_assets().await
    }

    pub(super) fn transfer_worker(&self) -> TransferWorker {
        TransferWorker {
            asset_storage_enabled: self.asset_storage_enabled.clone(),
            http_client: self.http_client.clone(),
            library_id: self.library_id,
            db_pool: self.database.clone(),
            assets: self.assets.clone(),
            event_tx: self.event_tx.clone(),
            credentials: Arc::clone(&self.credentials),
            account: self.account.clone(),
            queue: Arc::clone(&self.transfer_queue),
            availability_changed: Arc::default(),
            notification_interest: self.notification_interest.clone(),
        }
    }

    pub(crate) fn begin_thumbnail_upload_batch(&self) -> asset_transfer::TransferProducer {
        self.transfer_queue.begin_producer(TransferJobKind::UploadThumbnail)
    }

    pub fn transfer_statuses(&self) -> Vec<crate::TransferStatus> {
        self.transfer_queue.statuses()
    }

    /// Flush accepted history snapshots before the application releases its libraries.
    pub(crate) fn flush_transfer_history(&self) -> std::io::Result<()> {
        self.transfer_queue.flush_history()
    }

    pub fn transfer_history(&self) -> Vec<crate::TransferStatus> {
        self.transfer_queue.history()
    }

    pub fn request_download(&self, content_hash: ContentHash) -> Result<(), Box<dyn std::error::Error>> {
        self.request_downloads(std::slice::from_ref(&content_hash))
    }

    /// Downloads one explicitly requested publication without waiting behind
    /// library-wide state or background asset reconciliation.
    pub async fn download_requested(&self, content_hash: ContentHash) -> SyncResult<()> {
        if !self.is_signed_in() {
            return Err(SyncError::AuthenticationRequired);
        }
        let _interest = self.notification_interest.work();
        let user = client_library::sync_state::get_sync_metadata(&*self.database.get()?, "bound_user_id")?;
        let assets = self.assets.clone();
        let job = self
            .database
            .command(library_runtime::storage_queue::Priority::Interactive, move |conn| {
                super::ensure_sync_account(conn, &user)?;
                Ok(Self::prepare_requested_download(&assets, conn, &content_hash))
            })
            .await??;
        let Some(job) = job else { return Ok(()) };
        let worker = self.transfer_worker();
        worker.handle_job(job).await?;
        // One explicit download is its own boundary.
        worker.flush_availability_changes();
        Ok(())
    }

    pub fn request_downloads(&self, content_hashes: &[ContentHash]) -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = self.database.get()?;
        client_library::assets::request_book_downloads(&mut conn, content_hashes, TransferOrigin::UserInitiated)?;
        drop(conn);
        if let Some(tx) = &self.event_tx {
            for content_hash in content_hashes {
                let _ = tx.try_send(LibraryEvent::DownloadStatusChanged { content_hash: *content_hash, state: crate::DownloadState::Queued });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
