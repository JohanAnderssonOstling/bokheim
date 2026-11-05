//! Account commands coordinated with library access for native and browser hosts.
mod client;
mod model;
mod network;
mod notifications;
mod storage;
use crate::executor::{sleep, timeout, BackendExecutor, BackendFuture, BackendTask};
use crate::management::LibraryService;
use crate::BackendError;
pub use account_client::AccountStatus;
pub use client::{AccountUpdates, Client};
use client_platform_runtime::connectivity::Connectivity;
use library_services::account::{LibraryAccount, LibraryAccountSession};
pub use model::{AccountSnapshot, ServerBookStorageUsage, ServerLibraryStorageUsage};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;
pub use storage::SessionStorage;
use tokio::sync::watch;

pub type AccountSession = LibraryAccountSession<SessionStorage>;

pub struct Service {
    account: AccountSession,
    executor: BackendExecutor,
    libraries: Arc<LibraryService>,
    task: Mutex<Option<BackendTask<()>>>,
}

impl Service {
    pub fn new(account: AccountSession, libraries: Arc<LibraryService>, executor: BackendExecutor, requests: watch::Receiver<()>) -> Arc<Self> {
        let task = start(&executor, &libraries, account.library_account(), requests);
        Arc::new(Self { account, libraries, executor, task: Mutex::new(Some(task)) })
    }
    pub fn shutdown(&self) {
        self.task.lock().unwrap_or_else(PoisonError::into_inner).take();
    }
    pub fn subscribe_status(&self) -> watch::Receiver<AccountStatus> {
        self.account.subscribe_status()
    }
    pub async fn account_status(&self) -> Result<AccountStatus, BackendError> {
        let account = self.account.clone();
        self.libraries.run(move |_| account.status().map_err(account_error)).await
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<(), BackendError> {
        let change = self.account.prepare_login(email, password).await.map_err(account_error)?;
        self.libraries.apply_account_change(change).await.map(|_| ())
    }

    pub async fn adopt_session(&self, session: account_client::Session) -> Result<(), BackendError> {
        self.libraries.apply_account_change(self.account.prepare_adoption(session)).await.map(|_| ())
    }

    pub async fn verify_email(&self, email: &str, pin: &str, password: &str) -> Result<(), BackendError> {
        let change = self.account.prepare_verification(email, pin, password).await.map_err(account_error)?;
        self.libraries.apply_account_change(change).await.map(|_| ())
    }

    pub async fn logout(&self) -> Result<(), BackendError> {
        self.logout_with_notification(|session| async move {
            match timeout(Duration::from_secs(8), account_client::logout(&session)).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => log::warn!("Failed to notify server about logout: {error}"),
                Err(_) => log::warn!("Server logout notification timed out"),
            }
        })
        .await
    }

    pub async fn delete_account(&self, email: &str, password: &str) -> Result<(), BackendError> {
        let change = self.account.prepare_deletion(email, password).await.map_err(account_error)?;
        self.libraries.apply_account_change(change).await.map(|_| ())
    }

    pub async fn logout_with_notification<F: BackendFuture<()>>(&self, notify: impl FnOnce(account_client::Session) -> F) -> Result<(), BackendError> {
        let session = self.libraries.apply_account_change(self.account.prepare_logout()).await?;
        if let Some(session) = session {
            // Revoke only the captured session. A subsequent login must not
            // wait for or be affected by this best-effort notification.
            self.executor.spawn_detached(notify(session));
        }
        Ok(())
    }

    pub async fn account_snapshot(&self) -> Result<AccountSnapshot, BackendError> {
        let started = web_time::Instant::now();
        let account = self.account_status().await?;
        let (state, storage_usage, server_storage_usage, server_book_storage_usage) = if self.current_account().await?.is_some() {
            let remote = self.libraries.refresh_remote_account_details().await?;
            (remote.state, Some(remote.storage_usage), Some(remote.server_storage_usage), remote.server_book_storage_usage)
        } else {
            (self.libraries.library_list_state(None, None).await?, None, None, None)
        };
        log::debug!("account snapshot took {:?}", started.elapsed());
        let pending_cloud_bytes = if account.signed_in() { self.libraries.pending_cloud_books().await.ok().and_then(|books| books.values().try_fold(0_u64, |bytes, book| bytes.checked_add(*book))) } else { None };
        Ok(AccountSnapshot { account, storage_usage, pending_cloud_bytes, sync_statuses: state.sync_statuses, server_storage_usage, server_book_storage_usage })
    }

    pub async fn current_account(&self) -> Result<Option<account_client::Session>, BackendError> {
        self.account.session().await.map_err(account_error)
    }
}

const MAINTENANCE_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const MAINTENANCE_DEBOUNCE: Duration = Duration::from_millis(100);

pub use library_services::account::{account_error, auth_error};

fn start(executor: &BackendExecutor, libraries: &Arc<LibraryService>, account: LibraryAccount, requests: watch::Receiver<()>) -> BackendTask<()> {
    let state = Arc::downgrade(libraries);
    let library_state = state.clone();
    let mut changes = account.subscribe();
    changes.borrow_and_update();
    let library_changes = changes.clone();
    let wake = libraries.config.account_wake.clone();
    let interest = libraries.config.notification_interest.clone();
    let runtime_state = state.clone();
    executor.spawn(async move {
        tokio::join!(notifications::run(account, wake, interest, runtime_state), run_maintenance(state, requests, changes), reconcile_account_changes(library_state, library_changes));
    })
}

async fn reconcile_account_changes(state: Weak<LibraryService>, mut changes: watch::Receiver<u64>) {
    while changes.changed().await.is_ok() {
        let Some(state) = state.upgrade() else { return };
        if let Err(error) = state.reconcile_account_libraries().await {
            log::warn!("could not update libraries after account change: {error}");
        }
    }
}

async fn run_maintenance(state: Weak<LibraryService>, requests: watch::Receiver<()>, changes: watch::Receiver<u64>) {
    let network = match network::observe() {
        Ok(network) => network,
        Err(error) => {
            log::warn!("Could not observe account connectivity: {error}");
            return;
        }
    };
    run_maintenance_with_network(state, requests, changes, network).await;
}

async fn run_maintenance_with_network(state: Weak<LibraryService>, mut requests: watch::Receiver<()>, mut changes: watch::Receiver<u64>, mut network: Connectivity) {
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
        let Some(backend) = state.upgrade() else { return };
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod maintenance_tests;
