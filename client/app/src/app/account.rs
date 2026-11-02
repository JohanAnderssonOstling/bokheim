//! Account state, discovery, queued writes, and the shared notification connection.

mod network;
mod notifications;

use std::sync::{Arc, Weak};
use std::time::Duration;

use super::core::*;
use crate::AccountSnapshot;
use crate::executor::{BackendExecutor, BackendFuture, BackendTask, sleep, timeout};
use crate::notification_interest::NotificationInterest;
use client_platform_runtime::connectivity::Connectivity;
use futures_util::{StreamExt, TryStreamExt, stream};
use library_backend::library::LibraryDirectory;
use library_backend::library::handle::{LibraryActorCommand, LibraryControl};
use library_model::CloudQuotaBook;
use library_backend::{LibraryCommand, PendingCommand, ReadyCommand};
use serde::{Deserialize, Serialize};

use sync_common::api::libraries::{CloudStorageChange, LibraryCloudStorage};
use sync_common::{BookId, LibraryId};
use tokio::sync::watch;

const MAINTENANCE_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const MAINTENANCE_DEBOUNCE: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AccountStatus {
    signed_in: bool,
    server_url: Option<String>,
    email: Option<String>,
}

impl AccountStatus {
    pub fn signed_out() -> Self {
        Self::default()
    }

    pub fn authenticated(server_url: impl Into<String>, email: impl Into<String>) -> Self {
        Self { signed_in: true, server_url: Some(server_url.into()), email: Some(email.into()) }
    }

    pub const fn signed_in(&self) -> bool {
        self.signed_in
    }

    pub fn server_url(&self) -> Option<&str> {
        self.server_url.as_deref()
    }

    pub fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServerLibraryStorageUsage {
    pub library_id: LibraryId,
    pub library_name: String,
    pub used_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServerBookStorageUsage {
    pub content_hash: BookId,
    pub title: String,
    pub used_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct LocalLibraryStorageUsage {
    pub library_id: LibraryId,
    pub library_name: String,
    pub used_bytes: u64,
    #[serde(default)]
    pub cloud_bytes: Option<u64>,
    #[serde(default)]
    pub cloud_copy_bytes: Option<u64>,
}

struct RemoteAccountDetails {
    state: LibraryListState,
    storage_usage: account_contract::StorageUsageResponse,
    server_storage_usage: Vec<ServerLibraryStorageUsage>,
    server_book_storage_usage: Option<Vec<ServerBookStorageUsage>>,
}

fn session_identity(session: &account_client::Session) -> String {
    format!("{}|{}", session.server_url(), session.user_id())
}

/// Owns the shared account connection and maintenance together. Both loops
/// are dropped when the returned task is dropped; maintenance holds weak state.
pub(super) fn start(
    executor: &BackendExecutor, state: &Weak<AppState>, account: DeviceAccountSession, wake: watch::Sender<()>, requests: watch::Receiver<()>, interest: NotificationInterest, libraries: Arc<LibraryDirectory>,
) -> BackendTask<()> {
    let account_state = state.clone();
    // Subscribe before spawning so login or token refresh cannot be missed.
    let mut changes = account.subscribe();
    changes.borrow_and_update();
    let library_changes = changes.clone();
    let library_state = state.clone();
    executor.spawn(async move {
        tokio::join!(notifications::run(account, wake, interest, libraries), run_maintenance(account_state, requests, changes), reconcile_account_changes(library_state, library_changes));
    })
}

async fn reconcile_account_changes(state: Weak<AppState>, mut changes: watch::Receiver<u64>) {
    while changes.changed().await.is_ok() {
        let Some(state) = state.upgrade() else { return };
        if let Err(error) = (AppBackend { state }).reconcile_account_libraries() {
            log::warn!("could not update libraries after account change: {error}");
        }
    }
}

async fn run_maintenance(state: Weak<AppState>, requests: watch::Receiver<()>, changes: watch::Receiver<u64>) {
    let network = match network::observe() {
        Ok(network) => network,
        Err(error) => {
            log::warn!("Could not observe account connectivity: {error}");
            return;
        }
    };
    run_maintenance_with_network(state, requests, changes, network).await;
}

async fn run_maintenance_with_network(state: Weak<AppState>, mut requests: watch::Receiver<()>, mut changes: watch::Receiver<u64>, mut network: Connectivity) {
    let mut retry = None;
    loop {
        let online = network.online();
        let ready = tokio::select! {
            _ = network.changed() => { retry = None; if !network.online() { continue; } true },
            ready = wait_for_maintenance(&mut requests, &mut changes, if online { retry } else { None }) => ready,
        };
        if !ready {
            return;
        }
        if !network.online() {
            continue;
        }
        let Some(state) = state.upgrade() else { return };
        let backend = AppBackend { state };
        let result = tokio::select! {
            _ = network.changed() => { retry = None; continue; },
            result = backend.run_account_maintenance() => result,
        };
        retry = if result.is_err() { Some(next_maintenance_retry(retry)) } else { None };
    }
}

fn next_maintenance_retry(previous: Option<Duration>) -> Duration {
    let maximum = if cfg!(feature = "kobo") { Duration::from_secs(15 * 60) } else { MAINTENANCE_RETRY_INTERVAL };
    previous.map_or(MAINTENANCE_RETRY_INTERVAL, |previous| previous.saturating_mul(2).min(maximum))
}

/// New events bypass a failed pass's retry wait. Changes during a running pass
/// remain unseen so the next wait schedules another pass.
async fn wait_for_maintenance(requests: &mut watch::Receiver<()>, changes: &mut watch::Receiver<u64>, retry: Option<Duration>) -> bool {
    let changed = tokio::select! {
        changed = requests.changed() => changed,
        changed = changes.changed() => changed,
        result = sleep(retry.unwrap_or_default()), if retry.is_some() => return result.is_ok(),
    };
    if changed.is_err() {
        return false;
    }
    if sleep(MAINTENANCE_DEBOUNCE).await.is_err() {
        return false;
    }
    requests.borrow_and_update();
    changes.borrow_and_update();
    true
}

/// An account request's error, with what the account client knows about it.
pub(super) fn auth_error(error: account_client::AuthError) -> BackendError {
    match error {
        account_client::AuthError::SignInRequired(reason) => BackendError::classified(Failure::SignInRequired, reason),
        account_client::AuthError::PasswordTooLong => BackendError::classified(Failure::PasswordTooLong, "password exceeds the maximum length"),
        account_client::AuthError::Http(error) if error.status() == Some(reqwest::StatusCode::UPGRADE_REQUIRED) => BackendError::classified(Failure::ClientUpgradeRequired, error.to_string()),
        account_client::AuthError::Wire(sync_common::transport::WireError::UnsupportedVersion(version)) => BackendError::classified(Failure::ClientUpgradeRequired, format!("unsupported account response protocol version {version}")),
        error => BackendError::operation(error),
    }
}

/// An account session's error, with what the account client knows about it.
pub(super) fn account_error(error: account_client::AccountSessionError) -> BackendError {
    match error {
        account_client::AccountSessionError::Authentication(error) => auth_error(error),
        account_client::AccountSessionError::Changed => BackendError::classified(Failure::AccountChanged, "account changed while the request was in progress"),
        error => BackendError::operation(error),
    }
}

impl AppBackend {
    pub(crate) fn account_status(&self) -> Result<AccountStatus, BackendError> {
        let mut account = self.current_account()?;
        if account.is_none() {
            account = self.state.account.restore_if_empty().map_err(account_error)?;
        }
        Ok(account.as_ref().map(|session| AccountStatus::authenticated(session.server_url().as_str(), session.email())).unwrap_or_else(AccountStatus::signed_out))
    }

    pub(crate) async fn login(&self, email: &str, password: &str) -> Result<(), BackendError> {
        let session = account_client::login(&crate::api_server_url(), email, password).await.map_err(auth_error)?;
        self.adopt_session(session)
    }

    pub(crate) fn adopt_session(&self, session: account_client::Session) -> Result<(), BackendError> {
        self.with_storage(move |this| {
            if let Some(account) = this.account_identity()? {
                this.state.registry.bind_unowned_libraries(&account).map_err(BackendError::operation)?;
            }
            let account = session_identity(&session);
            this.retire_other_account_libraries(Some(&account))?;
            if let Err(error) = this.state.account.set(session).map_err(account_error) {
                this.reconcile_account_libraries()?;
                return Err(error);
            }
            this.state.registry.bind_unowned_libraries(&account).map_err(BackendError::operation)?;
            this.reconcile_account_libraries()
        })
    }

    pub(crate) async fn verify_email(&self, email: &str, pin: &str, password: &str) -> Result<(), BackendError> {
        let session = account_client::verify_email(&crate::api_server_url(), email, pin, password).await.map_err(auth_error)?;
        self.adopt_session(session)
    }

    pub(crate) fn logout(&self) -> Result<(), BackendError> {
        self.logout_with_notification(|session| async move {
            match timeout(Duration::from_secs(8), account_client::logout(&session)).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => log::warn!("Failed to notify server about logout: {error}"),
                Err(_) => log::warn!("Server logout notification timed out"),
            }
        })
    }

    pub(crate) async fn delete_account(&self, email: &str, password: &str) -> Result<(), BackendError> {
        let session = self.require_account()?;
        if session.email() != email {
            return Err(BackendError::classified(Failure::AccountChanged, "the signed-in account is not the one being deleted"));
        }
        account_client::delete_account(&session, password).await.map_err(auth_error)?;
        self.with_storage(move |this| {
            this.state.account.clear_user(session.user_id()).map_err(account_error)?;
            this.reconcile_account_libraries()
        })
    }

    pub(super) fn logout_with_notification<F: BackendFuture<()>>(&self, notify: impl FnOnce(account_client::Session) -> F) -> Result<(), BackendError> {
        let session = self.with_storage(move |this| {
            this.retire_other_account_libraries(None)?;
            let session = this.state.account.take().map_err(account_error)?;
            this.reconcile_account_libraries()?;
            Ok(session)
        })?;
        if let Some(session) = session {
            // Revoke only the captured session. A subsequent login must not
            // wait for or be affected by this best-effort notification.
            self.state.executor.spawn_detached(notify(session));
        }
        Ok(())
    }

    /// Fetches account facts only for an explicit caller. Nothing from this
    /// result is retained in AppState; the UI owns any presentation cache.
    async fn refresh_remote_account_details(&self) -> Result<RemoteAccountDetails, BackendError> {
        let account = self.account_identity()?;
        let expected_account = account.clone();
        let (snapshot, policies, book_storage) = tokio::try_join!(
            async {
                let started = web_time::Instant::now();
                let result = self.state.account.request(|session| Box::pin(account_client::account_snapshot(session))).await.map_err(account_error);
                log::debug!("account snapshot request took {:?}", started.elapsed());
                result
            },
            async {
                let started = web_time::Instant::now();
                let result = self.state.account.request(|session| Box::pin(account_client::cloud_storage(session))).await.map_err(account_error);
                log::debug!("account cloud storage request took {:?}", started.elapsed());
                result
            },
            async {
                let result = self
                    .state
                    .account
                    .request(|session| {
                        let authorized = expected_account.as_deref() == Some(session_identity(session).as_str());
                        Box::pin(async move {
                            if !authorized {
                                return Err(account_client::AuthError::SignInRequired("the account changed while loading book storage"));
                            }
                            account_client::book_storage_usage(session).await
                        })
                    })
                    .await;
                // Older servers can still provide the account and library view.
                let books = result.map(Some).unwrap_or_else(|error| {
                    log::debug!("account book storage details unavailable: {error}");
                    None
                });
                Ok::<_, BackendError>(books)
            },
        )?;
        let discovered = snapshot.libraries.clone();
        let discovery_account = account.clone();
        self.with_storage(move |this| {
            this.check_account_identity(&discovery_account)?;
            // This also identifies pending deletions from older installations,
            // which are excluded from the normal attachment below.
            for library in discovered {
                let already_bound = this.state.registry.accessible_to_account(&library.library_id, discovery_account.as_deref()).map_err(BackendError::operation)?;
                this.bind_library_to_current_account(&library.library_id)?;
                if !already_bound {
                    this.state.library_directory.request_remote_refresh(&library.library_id)?;
                    this.state.account_wake.send_replace(());
                }
            }
            Ok(())
        })?;
        self.apply_remote_library_deletions(&snapshot.deleted_library_ids, &account).await?;
        let session = self.require_account()?;
        // Settings is read-only and must not flush queued deletions. Keep those
        // libraries detached until the synchronization worker publishes the
        // deletion instead of resurrecting them from the remote snapshot.
        let pending_deletions = self.state.registry.pending_server_deletions().map_err(BackendError::operation)?.into_iter().collect::<HashSet<_>>();
        let libraries = snapshot.libraries.into_iter().filter(|library| !pending_deletions.contains(&library.library_id)).collect::<Vec<_>>();
        crate::platform::reserve_database_capacity(libraries.len()).await.map_err(BackendError::message)?;
        let mut library_storage_usage = snapshot
            .library_storage
            .into_iter()
            .map(|entry| {
                let library_id = entry.library_id.parse().map_err(BackendError::operation)?;
                let library_name = libraries.iter().find(|library| library.library_id == library_id).map(|library| library.library_name.clone()).unwrap_or_else(|| "Remote library".to_owned());
                Ok(ServerLibraryStorageUsage { library_id, library_name, used_bytes: entry.used_bytes })
            })
            .collect::<Result<Vec<_>, BackendError>>()?;
        library_storage_usage.sort_by(|a, b| b.used_bytes.cmp(&a.used_bytes).then_with(|| a.library_name.to_lowercase().cmp(&b.library_name.to_lowercase())).then_with(|| a.library_id.cmp(&b.library_id)));
        let storage_usage = snapshot.storage;
        // Publish remote knowledge before discovery emits library-list events.
        let remote_ids = self.with_storage(move |this| {
            this.check_account_identity(&account)?;
            let remote_ids = this.attach_remote_libraries(&libraries)?;
            this.apply_cloud_storage(&policies)?;
            Ok(remote_ids)
        })?;
        let server_book_storage_usage = match book_storage {
            Some(books) => Some(self.resolve_server_book_storage(books).await?),
            None => None,
        };
        self.check_account_identity(&expected_account)?;
        Ok(RemoteAccountDetails { state: self.library_list_state(Some(&session), Some(&remote_ids)).await?, storage_usage, server_storage_usage: library_storage_usage, server_book_storage_usage })
    }

    async fn resolve_server_book_storage(&self, books: Vec<account_contract::BookStorageUsage>) -> Result<Vec<ServerBookStorageUsage>, BackendError> {
        let hashes = books.iter().map(|book| book.content_hash).collect::<Vec<_>>();
        let mut titles = std::collections::HashMap::new();
        if !hashes.is_empty() {
            for entry in self.libraries()? {
                let Some(library) = self.state.library_directory.try_get(entry.library_id())? else { continue };
                match library.dispatch(LibraryCommand::Ready(ReadyCommand::BookStorageTitles { books: hashes.clone() })).await {
                    Ok(reply) => {
                        for (hash, title) in reply.take::<Vec<(BookId, String)>>()? {
                            if !title.trim().is_empty() {
                                titles.entry(hash).or_insert(title);
                            }
                        }
                    }
                    Err(error) => log::debug!("book titles unavailable for {}: {error}", entry.library_id()),
                }
                if titles.len() == hashes.len() {
                    break;
                }
            }
        }
        let mut books =
            books.into_iter().map(|book| ServerBookStorageUsage { content_hash: book.content_hash, title: titles.remove(&book.content_hash).unwrap_or_else(|| "Title unavailable".into()), used_bytes: book.used_bytes }).collect::<Vec<_>>();
        books.sort_by(|a, b| b.used_bytes.cmp(&a.used_bytes).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())).then_with(|| a.content_hash.cmp(&b.content_hash)));
        Ok(books)
    }

    async fn flush_pending_library_deletions(&self) -> Result<bool, BackendError> {
        let mut all_flushed = true;
        for library_id in self.state.registry.pending_server_deletions().map_err(BackendError::operation)? {
            if self.state.registry.device_only(&library_id).map_err(BackendError::operation)? {
                continue;
            }
            if !self.state.registry.accessible_to_account(&library_id, self.account_identity()?.as_deref()).map_err(BackendError::operation)? {
                continue;
            }
            match self
                .state
                .account
                .request(|session| {
                    let authorized = self.check_library_account(&library_id, session);
                    Box::pin(async move {
                        authorized?;
                        account_client::delete_library(session, &library_id).await
                    })
                })
                .await
            {
                Ok(_) => {
                    if let Err(error) = self.state.registry.complete_server_deletion(&library_id) {
                        all_flushed = false;
                        log::warn!("server deleted library {library_id}, but its pending tombstone could not be cleared locally: {error}");
                    }
                }
                Err(error) => {
                    all_flushed = false;
                    log::warn!("library {library_id} remains queued for server deletion: {error}");
                }
            }
        }
        Ok(all_flushed)
    }

    async fn apply_remote_library_deletions(&self, deleted: &[LibraryId], account: &Option<String>) -> Result<(), BackendError> {
        let local_ids = self.libraries()?.into_iter().map(|entry| *entry.library_id()).collect::<HashSet<_>>();
        for library_id in deleted.iter().filter(|library_id| local_ids.contains(library_id)) {
            let id = *library_id;
            let account = account.clone();
            self.with_storage(move |this| {
                this.check_account_identity(&account)?;
                this.state.registry.begin_local_removal(&id).map_err(BackendError::operation)?;
                this.state.library_directory.retire(&id)?;
                this.state.registry.complete_server_deletion(&id).map_err(BackendError::operation)
            })?;
            self.send_library_list_changed();
            self.finish_local_purge(id).await?;
        }
        Ok(())
    }

    pub(super) fn attach_remote_libraries(&self, remote: &[sync_common::api::libraries::LibrarySummary]) -> Result<HashSet<LibraryId>, BackendError> {
        let remote = remote.to_vec();
        let (remote_ids, needing_open, changed) = self.with_storage(move |this| {
            let mut remote_ids = HashSet::with_capacity(remote.len());
            let mut needing_open = Vec::new();
            let mut changed = false;
            for library in &remote {
                this.bind_library_to_current_account(&library.library_id)?;
                let existing = this.state.registry.entry(&library.library_id).map_err(BackendError::operation)?;
                remote_ids.insert(library.library_id);
                if this.state.registry.is_detached(&library.library_id).map_err(BackendError::operation)? {
                    continue;
                }
                // A stale server snapshot must not undo an offline rename.
                if this.state.registry.read_value(&format!("pending-library-rename/{}", library.library_id)).map_err(BackendError::operation)?.is_some() {
                    continue;
                }
                let renamed = existing.as_ref().map(|entry| entry.library_name()) != Some(library.library_name.as_str());
                if renamed {
                    changed = true;
                }
                // Library-list reconciliation updates registry metadata only. A live
                // Library owns expensive shared resources (pool, scanner, sync manager,
                // event forwarding) and must survive metadata refreshes unchanged.
                this.state.registry.attach_remote(&library.library_id, &library.library_name).map_err(BackendError::operation)?;
                this.state.registry.cache_remote_storage(&library.library_id, library.storage.clone()).map_err(BackendError::operation)?;

                if renamed {
                    if let Ok(handle) = this.state.library_directory.get(&library.library_id) {
                        let _ = handle.commands.try_send(LibraryActorCommand::Control(LibraryControl::Rename { name: library.library_name.clone(), refresh_remote: false }));
                    }
                }
                if this.state.library_directory.try_get(&library.library_id)?.is_none() {
                    needing_open.push(library.library_id);
                }
            }
            Ok((remote_ids, needing_open, changed))
        })?;
        // Each open rechecks registry policy in its own storage command. A
        // failed open leaves the library listed but unavailable.
        for library_id in needing_open {
            if let Err(error) = self.open_registered_library(&library_id) {
                log::warn!("Could not open registered library {library_id} during discovery: {error}");
            }
        }
        if changed {
            self.send_library_list_changed();
        }
        Ok(remote_ids)
    }

    pub(super) async fn library_list_state(&self, session: Option<&account_client::Session>, remote: Option<&HashSet<LibraryId>>) -> Result<LibraryListState, BackendError> {
        let libraries = self.libraries()?;
        let started = web_time::Instant::now();
        let sync_statuses = stream::iter(libraries.iter().cloned().map(|library| async move { self.library_sync_status(&library, session, remote).await })).buffered(4).try_collect().await?;
        log::debug!("account library statuses took {:?}", started.elapsed());
        Ok(LibraryListState { libraries, sync_statuses })
    }

    pub(super) async fn library_sync_status(&self, library: &LibraryEntry, session: Option<&account_client::Session>, remote: Option<&HashSet<LibraryId>>) -> Result<LibrarySyncStatus, BackendError> {
        let account = session.map(session_identity);
        let belongs_to_account = self.state.registry.accessible_to_account(library.library_id(), account.as_deref()).map_err(BackendError::operation)?;
        let device_only = self.state.registry.device_only(library.library_id()).map_err(BackendError::operation)?;
        if let Some(handle) = self.state.library_directory.try_get(library.library_id())? {
            match handle
                .dispatch(LibraryCommand::Ready(ReadyCommand::SyncStatus { signed_in: session.is_some() && belongs_to_account && !device_only, remote_present: remote.map(|remote| remote.contains(library.library_id())) }))
                .await
                .and_then(client_runtime::reply::Reply::take::<LibrarySyncStatus>)
            {
                Ok(mut status) => {
                    if session.is_some() && !belongs_to_account {
                        status.state = LibrarySyncState::AccountRequired;
                    }
                    return Ok(status);
                }
                Err(error) => log::warn!("Could not read sync status for library {}: {error}", library.library_id()),
            }
        }
        let mut status = LibrarySyncStatus::placeholder(*library.library_id(), LibrarySyncState::Unavailable);
        status.asset_storage_enabled = self.library_asset_storage_enabled(library.library_id())?;
        Ok(status)
    }

    /// Direct account-and-library snapshot for a foreground view. Expensive
    /// account discovery happens only because that view asked for it.
    pub(crate) async fn account_snapshot(&self) -> Result<AccountSnapshot, BackendError> {
        let started = web_time::Instant::now();
        let account = self.account_status()?;
        let (state, storage_usage, server_storage_usage, server_book_storage_usage) = if self.current_account()?.is_some() {
            let remote = self.refresh_remote_account_details().await?;
            (remote.state, Some(remote.storage_usage), Some(remote.server_storage_usage), remote.server_book_storage_usage)
        } else {
            (self.library_list_state(None, None).await?, None, None, None)
        };
        log::debug!("account snapshot took {:?}", started.elapsed());
        let pending_cloud_bytes = if account.signed_in() { self.pending_cloud_books().await.ok().and_then(|books| books.values().try_fold(0_u64, |bytes, book| bytes.checked_add(*book))) } else { None };
        Ok(AccountSnapshot { account, storage_usage, pending_cloud_bytes, sync_statuses: state.sync_statuses, server_storage_usage, server_book_storage_usage })
    }

    pub(crate) async fn local_storage_usage(&self) -> Result<Vec<LocalLibraryStorageUsage>, BackendError> {
        let libraries = self.libraries()?;
        let started = web_time::Instant::now();
        let entries: Vec<Option<LocalLibraryStorageUsage>> = stream::iter(libraries.iter().map(|entry| async move {
            let Ok(library) = self.state.library_directory.get(entry.library_id()) else {
                return Ok::<_, BackendError>(None);
            };
            let books: Vec<CloudQuotaBook> = library.dispatch(LibraryCommand::Ready(ReadyCommand::CloudQuotaBooks)).await?.take()?;
            let cloud_bytes = books.iter().try_fold(0_u64, |bytes, book| bytes.checked_add(book.size_bytes));
            let cloud_copy_bytes = self.cloud_copy_remaining_bytes(*entry.library_id(), &books)?;
            Ok(library.dispatch(LibraryCommand::Pending(PendingCommand::LocalStorageUsage)).await?.take::<Option<u64>>()?.map(|used_bytes| LocalLibraryStorageUsage {
                library_id: *entry.library_id(),
                library_name: entry.library_name().to_owned(),
                used_bytes,
                cloud_bytes,
                cloud_copy_bytes,
            }))
        }))
        .buffered(4)
        .try_collect()
        .await?;
        let mut usage: Vec<_> = entries.into_iter().flatten().collect();
        usage.sort_by(|left, right| left.library_name.cmp(&right.library_name));
        log::debug!("account local storage usage took {:?}", started.elapsed());
        Ok(usage)
    }

    pub(super) fn require_account(&self) -> Result<account_client::Session, BackendError> {
        self.state.account.require().map_err(account_error)
    }

    pub(super) fn current_account(&self) -> Result<Option<account_client::Session>, BackendError> {
        self.state.account.current().map_err(account_error)
    }

    async fn publish_pending_library_names(&self) -> Result<(), BackendError> {
        if self.current_account()?.is_none() {
            return Ok(());
        }
        let mut failure = None;
        for entry in self.libraries()? {
            let id = *entry.library_id();
            if self.state.registry.device_only(&id).map_err(BackendError::operation)? {
                continue;
            }
            if !self.state.registry.accessible_to_account(&id, self.account_identity()?.as_deref()).map_err(BackendError::operation)? {
                continue;
            }
            let result = async {
                let key = format!("pending-library-rename/{id}");
                let Some(pending) = self.state.registry.read_value(&key).map_err(BackendError::operation)? else {
                    return Ok::<(), BackendError>(());
                };
                let name = String::from_utf8(pending.clone()).map_err(BackendError::operation)?;
                self.state
                    .account
                    .request(|session| {
                        let authorized = self.check_library_account(&id, session);
                        let name = name.clone();
                        Box::pin(async move {
                            authorized?;
                            account_client::rename_library(session, id, name).await
                        })
                    })
                    .await
                    .map_err(account_error)?;
                self.with_storage(move |this| {
                    // A second rename during the request must remain queued.
                    if this.state.registry.read_value(&key).map_err(BackendError::operation)?.as_ref() == Some(&pending) {
                        this.state.registry.remove_value(&key).map_err(BackendError::operation)?;
                    }
                    Ok(())
                })
            }
            .await;
            if let Err(error) = result {
                log::warn!("rename for library {id} remains queued: {error}");
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    async fn run_account_maintenance(&self) -> Result<(), ()> {
        let result = async {
            let account = self.account_identity()?;
            if account.is_none() {
                return Ok(());
            }
            let (writes, refresh) = tokio::join!(self.flush_account_writes_cycle(), self.refresh_remote_account_details());
            writes?;
            refresh?;
            self.check_account_identity(&account)
        }
        .await;
        result.map_err(|error| log::warn!("account discovery will retry: {error}"))
    }

    async fn flush_account_writes_cycle(&self) -> Result<(), BackendError> {
        let mut changed = self.state.account.subscribe();
        changed.borrow_and_update();
        let work = async {
            if self.current_account()?.is_none() {
                return Ok(());
            }
            // Every queue gets an attempt even if another fails.
            let (names, deletions, cloud) = tokio::join!(self.publish_pending_library_names(), self.flush_pending_library_deletions(), self.publish_pending_cloud_storage());
            // Feed changes made offline; the catalog page also syncs them.
            #[cfg(not(target_arch = "wasm32"))]
            if let Err(error) = self.sync_opds_feeds().await {
                log::warn!("OPDS feed changes remain queued: {error}");
            }
            cloud?;
            names?;
            if deletions? { Ok(()) } else { Err(BackendError::message("library deletions remain pending")) }
        };
        tokio::select! {
            biased;
            _ = changed.changed() => Ok(()),
            result = work => result,
        }
    }

    pub(super) fn account_identity(&self) -> Result<Option<String>, BackendError> {
        Ok(self.current_account()?.as_ref().map(session_identity))
    }

    pub(super) fn bind_library_to_current_account(&self, id: &LibraryId) -> Result<(), BackendError> {
        if self.state.registry.device_only(id).map_err(BackendError::operation)? {
            return Ok(());
        }
        if let Some(account) = self.account_identity()? {
            self.state.registry.bind_library_account(id, &account).map_err(BackendError::operation)?;
        }
        Ok(())
    }

    pub(super) fn library_account(&self, id: LibraryId) -> library_backend::integration::LibraryAccount {
        let registry = self.state.registry.clone();
        self.state.account.library_account().with_authorization(move |session| {
            if registry.device_only(&id).map_err(|error| std::io::Error::other(error.to_string()))? {
                return Ok(false);
            }
            let account = session_identity(session);
            if !registry.accessible_to_account(&id, Some(&account)).map_err(|error| std::io::Error::other(error.to_string()))? {
                return Ok(false);
            }
            registry.bind_library_account(&id, &account).map_err(|error| std::io::Error::other(error.to_string()))?;
            Ok(true)
        })
    }

    fn retire_other_account_libraries(&self, account: Option<&str>) -> Result<(), BackendError> {
        for library in self.state.library_directory.all()? {
            if !self.state.registry.accessible_to_account(&library.id(), account).map_err(BackendError::operation)? {
                self.state.library_directory.retire(&library.id())?;
            }
        }
        Ok(())
    }

    fn reconcile_account_libraries(&self) -> Result<(), BackendError> {
        self.with_storage(move |this| {
            this.retire_other_account_libraries(this.account_identity()?.as_deref())?;
            this.open_registered_libraries()?;
            this.send_library_list_changed();
            Ok(())
        })
    }

    fn check_library_account(&self, id: &LibraryId, session: &account_client::Session) -> Result<(), account_client::AuthError> {
        if self.state.registry.device_only(id).map_err(|error| account_client::AuthError::InvalidData(error.to_string()))? {
            return Err(account_client::AuthError::SignInRequired("this library is kept only on this device"));
        }
        if self.state.registry.accessible_to_account(id, Some(&session_identity(session))).map_err(|error| account_client::AuthError::InvalidData(error.to_string()))? {
            Ok(())
        } else {
            Err(account_client::AuthError::SignInRequired("sign in to the account that owns this library"))
        }
    }

    fn check_account_identity(&self, account: &Option<String>) -> Result<(), BackendError> {
        if self.account_identity()? != *account {
            return Err(BackendError::message("account changed during background synchronization"));
        }
        Ok(())
    }

    async fn publish_cloud_storage(&self, id: LibraryId) -> Result<(), BackendError> {
        if self.state.registry.device_only(&id).map_err(BackendError::operation)? {
            return Ok(());
        }
        if !self.state.registry.accessible_to_account(&id, self.account_identity()?.as_deref()).map_err(BackendError::operation)? {
            return Ok(());
        }
        let key = format!("pending-cloud-storage:{id}");
        let Some(pending) = self.state.registry.read_value(&key).map_err(BackendError::operation)? else { return Ok(()) };
        let text = std::str::from_utf8(&pending).map_err(BackendError::operation)?;
        let (change_id, value) = text.split_once(':').ok_or_else(|| BackendError::message("Invalid pending cloud storage change"))?;
        let change = CloudStorageChange { change_id: change_id.parse().map_err(BackendError::operation)?, enabled: value == "1" };
        let account = self.account_identity()?;
        let result = self
            .state
            .account
            .request(|session| {
                let authorized = self.check_library_account(&id, session);
                let change = change.clone();
                Box::pin(async move {
                    authorized?;
                    account_client::set_cloud_storage(session, id, change).await
                })
            })
            .await
            .map_err(account_error)?;
        self.with_storage(move |this| {
            this.check_account_identity(&account)?;
            if this.state.registry.read_value(&key).map_err(BackendError::operation)?.as_ref() == Some(&pending) {
                this.state.registry.cache_cloud_storage_with_reset(&id, result.enabled).map_err(BackendError::operation)?;
                this.apply_open_cloud_storage(id, result.enabled)?;
                this.state.registry.remove_value(&key).map_err(BackendError::operation)?;
            }
            this.state.account_wake.send_replace(());
            Ok(())
        })
    }

    fn apply_cloud_storage(&self, states: &[LibraryCloudStorage]) -> Result<(), BackendError> {
        let states = states.to_vec();
        self.with_storage(move |this| {
            for state in &states {
                let id = state.library_id;
                if this.state.registry.entry(&id).map_err(BackendError::operation)?.is_none() || this.state.registry.read_value(&format!("pending-cloud-storage:{id}")).map_err(BackendError::operation)?.is_some() {
                    continue;
                }
                if this.library_asset_storage_enabled(&id)? != state.enabled {
                    this.state.registry.cache_cloud_storage_with_reset(&id, state.enabled).map_err(BackendError::operation)?;
                    this.apply_open_cloud_storage(id, state.enabled)?;
                }
            }
            Ok(())
        })
    }

    fn apply_open_cloud_storage(&self, id: LibraryId, enabled: bool) -> Result<(), BackendError> {
        if let Ok(library) = self.state.library_directory.get(&id) {
            let _ = library.commands.try_send(LibraryActorCommand::Control(LibraryControl::SetAssetStorage { enabled, reset_cloud_presence: true }));
        }
        Ok(())
    }

    async fn publish_pending_cloud_storage(&self) -> Result<(), BackendError> {
        let mut failure = None;
        for entry in self.libraries()? {
            let id = *entry.library_id();
            if self.state.registry.read_value(&format!("pending-cloud-storage:{id}")).map_err(BackendError::operation)?.is_none() {
                continue;
            }
            if let Err(error) = self.publish_cloud_storage(id).await {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod storage_tests {
    use super::*;
    use account_contract::BookStorageUsage;
    use library_backend::LibraryClient;
    use library_model::BlobKind;
    use sync_common::{BookId, ROOT_DIR_ID};

    #[tokio::test]
    async fn account_book_storage_uses_titles_without_downloads_and_ranks_cloud_sizes() {
        let root = tempfile::tempdir().unwrap();
        let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
        let backend = AppBackend::new(context).unwrap();
        let entry = backend.create_library_with_asset_storage("Local cache".into(), false).unwrap();
        let id = *entry.library_id();
        let library = LibraryClient::new(id, backend.state.library_directory.get(&id).unwrap());
        let large_bytes = include_bytes!("../../../library-backend/tests/fixtures/cloud-cover.epub");
        let small_bytes = include_bytes!("../../../library-backend/crates/book-inspection/tests/fixtures/malformed-language.epub");
        let large = library.import_downloaded_book(ROOT_DIR_ID, "Large.epub".into(), large_bytes.to_vec()).await.unwrap();
        let small = library.import_downloaded_book(ROOT_DIR_ID, "Small.epub".into(), small_bytes.to_vec()).await.unwrap();
        backend.state.library_directory.retire(&id).unwrap();
        {
            let db = library_database::Database::open(&backend.state.registry.library_database_locator(&id, entry.storage_locator())).unwrap();
            db.record_remote_asset(BlobKind::Book, &large).unwrap();
            db.evict_local_book(&large).unwrap();
        }
        std::fs::remove_file(Path::new(entry.storage_locator()).join("Large.epub")).unwrap();
        backend.open_registered_library(&id).unwrap();
        let unknown = BookId::from_bytes([7; 32]);
        let books = backend
            .resolve_server_book_storage(vec![
                BookStorageUsage { content_hash: small, used_bytes: small_bytes.len() as u64 },
                BookStorageUsage { content_hash: unknown, used_bytes: 1 },
                BookStorageUsage { content_hash: large, used_bytes: large_bytes.len() as u64 },
            ])
            .await
            .unwrap();
        assert_eq!(books.iter().map(|book| book.content_hash).collect::<Vec<_>>(), [large, small, unknown]);
        assert_eq!(books[0].title, "Cloud cover fixture");
        assert_eq!(books[1].title, "Language metadata test");
        assert_eq!(books[2].title, "Title unavailable");
        assert!(!Path::new(entry.storage_locator()).join("Large.epub").exists(), "reading titles must not download book files");
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod maintenance_tests {
    use super::*;
    use futures_util::poll;
    use tokio::time::advance;

    #[tokio::test(start_paused = true)]
    async fn offline_maintenance_parks_without_retaining_application_state() {
        let (sender, requests) = watch::channel(());
        let (_account_sender, changes) = watch::channel(0);
        let (connection, status) = watch::channel(false);
        let mut maintenance = Box::pin(run_maintenance_with_network(Weak::new(), requests, changes, Connectivity::new(status)));
        assert!(poll!(&mut maintenance).is_pending());
        sender.send_replace(());
        assert!(poll!(&mut maintenance).is_pending());
        advance(MAINTENANCE_DEBOUNCE).await;
        assert!(poll!(&mut maintenance).is_pending());
        advance(Duration::from_secs(3600)).await;
        assert!(poll!(&mut maintenance).is_pending());
        // Reconnection must be sufficient to schedule work without a new UI event.
        // No AppState remains here, so the worker exits immediately.
        connection.send_replace(true);
        maintenance.await;
    }

    #[tokio::test(start_paused = true)]
    async fn closing_the_owner_while_offline_ends_maintenance() {
        let (sender, requests) = watch::channel(());
        let (_account_sender, changes) = watch::channel(0);
        let (_connection, status) = watch::channel(false);
        let mut maintenance = Box::pin(run_maintenance_with_network(Weak::new(), requests, changes, Connectivity::new(status)));
        assert!(poll!(&mut maintenance).is_pending());
        drop(sender);
        maintenance.await;
    }

    #[cfg(feature = "kobo")]
    #[test]
    fn repeated_account_failures_back_off_to_fifteen_minutes() {
        let mut delay = None;
        for seconds in [30, 60, 120, 240, 480, 900, 900] {
            delay = Some(next_maintenance_retry(delay));
            assert_eq!(delay, Some(Duration::from_secs(seconds)));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn failure_retries_after_thirty_seconds_without_an_event() {
        let (_sender, mut requests) = watch::channel(());
        let (_account_sender, mut changes) = watch::channel(0);
        let mut waiting = Box::pin(wait_for_maintenance(&mut requests, &mut changes, Some(MAINTENANCE_RETRY_INTERVAL)));
        assert!(poll!(&mut waiting).is_pending());
        advance(Duration::from_secs(29)).await;
        assert!(poll!(&mut waiting).is_pending());
        advance(Duration::from_secs(1)).await;
        assert!(waiting.await);
    }

    #[tokio::test(start_paused = true)]
    async fn new_events_bypass_retry_wait_and_coalesce_during_debounce() {
        let (sender, mut requests) = watch::channel(());
        let (_account_sender, mut changes) = watch::channel(0);
        let mut waiting = Box::pin(wait_for_maintenance(&mut requests, &mut changes, Some(MAINTENANCE_RETRY_INTERVAL)));
        assert!(poll!(&mut waiting).is_pending());
        sender.send_replace(());
        assert!(poll!(&mut waiting).is_pending());
        sender.send_replace(());
        advance(Duration::from_millis(99)).await;
        assert!(poll!(&mut waiting).is_pending());
        advance(Duration::from_millis(1)).await;
        assert!(waiting.await);
        assert!(!requests.has_changed().unwrap());

        // An event raised by a running pass must still schedule the next pass.
        sender.send_replace(());
        let mut waiting = Box::pin(wait_for_maintenance(&mut requests, &mut changes, None));
        assert!(poll!(&mut waiting).is_pending());
        advance(MAINTENANCE_DEBOUNCE).await;
        assert!(waiting.await);
    }

    #[tokio::test(start_paused = true)]
    async fn session_changes_wake_maintenance_without_a_notification_connection() {
        for retry in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
            let server = "https://example.com".parse().unwrap();
            let account = DeviceAccountSession::load(context, server).unwrap();
            let mut changes = account.subscribe();
            let (_sender, mut requests) = watch::channel(());
            let mut waiting = Box::pin(wait_for_maintenance(&mut requests, &mut changes, retry.then_some(MAINTENANCE_RETRY_INTERVAL)));
            assert!(poll!(&mut waiting).is_pending());

            let session = account_client::Session::try_new("https://example.com".parse().unwrap(), "user".into(), "reader@example.com".into(), "a".repeat(43)).unwrap();
            account.set(session.clone()).unwrap();
            assert!(poll!(&mut waiting).is_pending());
            account.set(session).unwrap();
            advance(MAINTENANCE_DEBOUNCE).await;
            assert!(waiting.await);
            assert!(!changes.has_changed().unwrap(), "session changes during debounce coalesce");
            assert!(!requests.has_changed().unwrap(), "maintenance does not need a forwarded wake");

            // A logout during a maintenance pass must schedule the next pass.
            account.take().unwrap();
            let mut waiting = Box::pin(wait_for_maintenance(&mut requests, &mut changes, None));
            assert!(poll!(&mut waiting).is_pending());
            advance(MAINTENANCE_DEBOUNCE).await;
            assert!(waiting.await);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn closing_wake_channel_stops_idle_and_retry_waits() {
        for retry in [false, true] {
            let (sender, mut requests) = watch::channel(());
            let (_account_sender, mut changes) = watch::channel(0);
            let mut waiting = Box::pin(wait_for_maintenance(&mut requests, &mut changes, retry.then_some(MAINTENANCE_RETRY_INTERVAL)));
            assert!(poll!(&mut waiting).is_pending());
            drop(sender);
            assert!(!waiting.await);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod unavailable_library_tests {
    use super::*;

    #[tokio::test]
    async fn unavailable_library_does_not_block_account_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).unwrap();
        let library = backend.create_library_with_asset_storage("Unavailable".to_owned(), false).unwrap();
        let id = *library.library_id();
        backend.state.library_directory.retire(&id).unwrap();

        let snapshot = backend.account_snapshot().await.unwrap();
        let status = snapshot.sync_statuses.iter().find(|status| status.library_id == id).unwrap();
        assert_eq!(status.state, LibrarySyncState::Unavailable);
        assert!(!status.asset_storage_enabled);
        assert!(backend.local_storage_usage().await.unwrap().iter().all(|usage| usage.library_id != id));
    }

    #[tokio::test]
    async fn discovery_reopens_an_existing_registered_library() {
        let root = tempfile::tempdir().unwrap();
        let backend = AppBackend::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap()).unwrap();
        let library = backend.create_library_with_asset_storage("Existing".to_owned(), true).unwrap();
        let id = *library.library_id();
        backend.state.library_directory.retire(&id).unwrap();

        let remote = [sync_common::api::libraries::LibrarySummary { library_id: id, library_name: "Existing".to_owned(), storage: Default::default() }];
        assert!(backend.attach_remote_libraries(&remote).unwrap().contains(&id));
        assert!(backend.state.library_directory.try_get(&id).unwrap().is_some());
    }
}
