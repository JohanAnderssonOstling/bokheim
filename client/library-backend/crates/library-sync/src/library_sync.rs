pub(crate) use super::error::SyncError;
pub(crate) use super::error::SyncResult;
pub(crate) use crate::engine::SyncOutcome;
pub(crate) use account_client::Url;
use asset_transfer::transfer::AssetExecutor;
pub(crate) use library_database::Database;
pub(crate) use library_model::{LibraryUpdate, TransferOrigin};
use library_placement::LibraryPlacement;
use library_runtime::account::{LibraryAccount, LibraryAuthentication, SharedSyncCredentials};
pub(crate) use library_runtime::events::{LibraryEvent, LibraryEventRouter};
pub(crate) use library_runtime::interest::NotificationInterest;
pub(crate) use std::sync::Arc;
pub(crate) use sync_common::BookId;
pub(crate) use sync_common::{LibraryId, ReplicaId};
use sync_transport::SyncCredentials;

#[cfg(not(target_arch = "wasm32"))]
#[path = "library_sync/native.rs"]
mod host;
#[cfg(target_arch = "wasm32")]
#[path = "library_sync/web.rs"]
mod host;

pub struct LibrarySyncConfig {
    pub placement: Arc<LibraryPlacement>,
    pub server_url: Url,
    pub library_id: LibraryId,
    pub authentication: LibraryAuthentication,
    pub notification_interest: NotificationInterest,
    pub cpu: Arc<dyn cpu_host::CpuHost>,
}

pub struct LibrarySync {
    pub(crate) database: Arc<Database>,
    pub(crate) replica_id: ReplicaId,
    pub(crate) server_url: Url,
    pub(crate) http_client: reqwest::Client,
    pub(crate) library_id: LibraryId,
    pub(crate) events: Option<LibraryEventRouter>,
    pub(crate) credentials: SharedSyncCredentials,
    pub(crate) account: Option<LibraryAccount>,
    pub(crate) notification_interest: NotificationInterest,
    #[cfg(not(target_arch = "wasm32"))]
    sync_gate: tokio::sync::Mutex<()>,
    asset_executor: AssetExecutor,
}

impl LibrarySync {
    pub fn new(config: LibrarySyncConfig) -> SyncResult<Self> {
        let database = config.placement.database().clone();
        let replica_id = database.sync_metadata("replica_id")?.ok_or_else(|| SyncError::failed("library database has not been initialized"))?.parse().map_err(SyncError::failed)?;
        let asset_executor = AssetExecutor::new(config.placement.clone(), config.library_id, config.authentication.clone(), config.notification_interest.clone(), config.cpu);
        Ok(Self {
            database,
            replica_id,
            server_url: config.server_url,
            http_client: asset_executor.http_client().clone(),
            library_id: config.library_id,
            events: Some(config.placement.events().clone()),
            credentials: config.authentication.credentials(),
            account: config.authentication.account(),
            notification_interest: config.notification_interest,
            #[cfg(not(target_arch = "wasm32"))]
            sync_gate: tokio::sync::Mutex::new(()),
            asset_executor,
        })
    }

    pub fn library_id(&self) -> LibraryId {
        self.library_id
    }

    pub fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }

    pub fn notification_interest(&self) -> &NotificationInterest {
        &self.notification_interest
    }

    pub fn is_signed_in(&self) -> bool {
        self.credentials.credentials().is_some()
    }

    /// Validates that the current session targets this library's server.
    /// Account switches are non-events, so no binding is stored or cleared.
    pub fn bind_account(&self, server_url: &str) -> Result<(), String> {
        if server_url != self.server_url.as_str() {
            return Err(format!("account belongs to {server_url}, but this library syncs with {}", self.server_url));
        }
        Ok(())
    }

    /// This handle observes the app-global account state. Workers adopt the
    /// current identity at execution time, avoiding account broadcasts.
    pub fn bind_current_account(&self) -> Result<bool, String> {
        let Some(account) = &self.account else {
            return Ok(self.is_signed_in());
        };
        let Some(session) = account.current().map_err(|error| error.to_string())? else {
            return Ok(false);
        };
        self.bind_account(session.server_url().as_str())?;
        Ok(true)
    }

    pub fn account(&self) -> Option<LibraryAccount> {
        self.account.clone()
    }

    /// Capture the user and credentials from one live session. Signed-out
    /// libraries skip the exchange before preparing any database work.
    fn sync_identity(&self) -> SyncResult<Option<(String, SyncCredentials)>> {
        let Some(account) = &self.account else {
            return Ok(self.credentials.credentials().map(|credentials| (String::new(), credentials)));
        };
        let Some(session) = account.current().map_err(SyncError::failed)? else { return Ok(None) };
        if session.server_url() != &self.server_url {
            return Err(SyncError::failed("account belongs to another server"));
        }
        Ok(Some((session.user_id().to_owned(), SyncCredentials::new(session.server_url().clone(), session.token()))))
    }

    pub async fn drain_outbox(&self) -> SyncResult<SyncOutcome> {
        self.synchronize_state_mode(true).await
    }

    pub async fn synchronize_state_mode(&self, outbox_only: bool) -> SyncResult<SyncOutcome> {
        #[cfg(not(target_arch = "wasm32"))]
        let _sync_guard = self.sync_gate.lock().await;
        let Some((user, credentials)) = self.sync_identity()? else { return Ok(SyncOutcome::default()) };
        let _interest = self.notification_interest.work();
        let outcome = host::exchange(self, &user, credentials, outbox_only).await?;
        // This checkpoint covers publishable state. Books awaiting upload keep
        // their mutations in the outbox; upload completion wakes another pass.
        if !outbox_only {
            let completed_at = u64::try_from(web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH)?.as_millis())?;
            self.database.set_sync_metadata("last_successful_sync_at", &completed_at.to_string()).map_err(SyncError::failed)?;
        }
        if let Some(events) = &self.events {
            events.publish_update(LibraryUpdate::SyncStatusChanged);
        }
        Ok(outcome)
    }

    pub fn set_asset_storage_enabled(&self, enabled: bool) {
        self.asset_executor.set_asset_storage_enabled(enabled);
    }

    pub fn asset_storage_enabled(&self) -> bool {
        self.asset_executor.asset_storage_enabled()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn request_download(&self, book_id: BookId) -> SyncResult<()> {
        self.request_downloads(std::slice::from_ref(&book_id))
    }

    pub fn request_downloads(&self, book_ids: &[BookId]) -> SyncResult<()> {
        self.database.sync_request_book_downloads(book_ids, TransferOrigin::UserInitiated)?;
        if let Some(tx) = &self.events {
            for book_id in book_ids {
                let _ = tx.emit(LibraryEvent::DownloadStatusChanged { book_id: *book_id, state: library_model::DownloadState::Queued });
            }
        }
        Ok(())
    }
}

impl LibrarySync {
    pub fn asset_executor(&self) -> AssetExecutor {
        self.asset_executor.clone()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "library_sync/tests.rs"]
mod tests;
