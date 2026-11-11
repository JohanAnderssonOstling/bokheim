pub(crate) use client_protocol::BackendError;
use super::error::SyncError;
pub(crate) use super::error::SyncResult;
pub(crate) use crate::engine::SyncOutcome;
pub(crate) use account_client::Url;
use library_database::DatabaseClient;
use library_model::LibraryUpdate;
use library_services::account::LibraryAuthentication;
pub(crate) use library_services::events::LibraryEventRouter;
pub(crate) use library_services::interest::NotificationInterest;
pub(crate) use sync_common::{LibraryId, ReplicaId};
use sync_transport::SyncCredentials;
use tokio::sync::Mutex;

#[path = "library_sync/native.rs"]
mod host;

pub struct LibrarySyncConfig {
    pub database: DatabaseClient,
    pub events: LibraryEventRouter,
    pub http_client: reqwest::Client,
    pub server_url: Url,
    pub library_id: LibraryId,
    pub authentication: LibraryAuthentication,
    pub notification_interest: NotificationInterest,
}

pub struct LibrarySync {
    pub(crate) replica_id: ReplicaId,
    pub(crate) server_url: Url,
    pub(crate) http_client: reqwest::Client,
    pub(crate) library_id: LibraryId,
    pub(crate) events: LibraryEventRouter,
    pub(crate) authentication: LibraryAuthentication,
    pub(crate) notification_interest: NotificationInterest,
    /// Protocol work uses the stable connection independently of file access.
    pub(crate) database: DatabaseClient,
    exchange: Mutex<()>,
}

impl LibrarySync {
    pub async fn new(config: LibrarySyncConfig) -> SyncResult<Self> {
        let database = config.database;
        let replica_id = database.sync_metadata("replica_id").await?.ok_or_else(|| SyncError::failed("library database has not been initialized"))?.parse().map_err(SyncError::failed)?;
        Ok(Self {
            replica_id,
            server_url: config.server_url,
            http_client: config.http_client,
            library_id: config.library_id,
            events: config.events,
            authentication: config.authentication,
            notification_interest: config.notification_interest,
            database,
            exchange: Mutex::new(()),
        })
    }

    pub fn library_id(&self) -> LibraryId {
        self.library_id
    }

    pub fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }

    pub async fn is_signed_in(&self) -> bool {
        self.authentication.credentials().await.is_some()
    }

    /// Validates that the current session targets this library's server.
    /// Account switches are non-events, so no binding is stored or cleared.
    pub fn bind_account(&self, server_url: &str) -> Result<(), String> {
        if server_url != self.server_url.as_str() {
            return Err(format!("account belongs to {server_url}, but this library syncs with {}", self.server_url));
        }
        Ok(())
    }

    /// Remote workers adopt the current identity only after management has
    /// confirmed server registration for that account.
    pub async fn remote_ready(&self) -> Result<bool, BackendError> {
        let Some(account) = self.authentication.account() else {
            return Ok(self.is_signed_in().await);
        };
        let Some(session) = account.session().await.map_err(BackendError::operation)? else {
            return Ok(false);
        };
        self.bind_account(session.server_url().as_str())?;
        Ok(self.database.sync_metadata("server_library_created").await.map_err(BackendError::operation)?.as_deref() == Some(session.identity().as_str()))
    }

    /// Capture the user and credentials from one live session. Signed-out
    /// libraries skip the exchange before preparing any database work.
    async fn sync_identity(&self) -> SyncResult<Option<(String, SyncCredentials)>> {
        let Some(account) = self.authentication.account() else {
            return Ok(self.authentication.credentials().await.map(|credentials| (String::new(), credentials)));
        };
        let Some(session) = account.session().await.map_err(SyncError::failed)? else { return Ok(None) };
        if session.server_url() != &self.server_url {
            return Err(SyncError::failed("account belongs to another server"));
        }
        Ok(Some((session.user_id().to_owned(), SyncCredentials::new(session.server_url().clone(), session.token()))))
    }

    pub async fn drain_outbox(&self) -> SyncResult<SyncOutcome> {
        self.synchronize_state_mode(true).await
    }

    pub async fn synchronize_state_mode(&self, outbox_only: bool) -> SyncResult<SyncOutcome> {
        let _exchange = self.exchange.lock().await;
        let Some((user, credentials)) = self.sync_identity().await? else { return Ok(SyncOutcome::default()) };
        let _interest = self.notification_interest.work();
        let outcome = host::exchange(self, &user, credentials, outbox_only).await?;
        // This checkpoint covers publishable state. Books awaiting upload keep
        // their mutations in the outbox; upload completion wakes another pass.
        if !outbox_only {
            let completed_at = u64::try_from(web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH)?.as_millis())?;
            self.database.set_sync_metadata("last_successful_sync_at", &completed_at.to_string()).await.map_err(SyncError::failed)?;
        }
        self.events.updates.publish(LibraryUpdate::SyncStatusChanged);
        Ok(outcome)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "library_sync/tests.rs"]
mod tests;
