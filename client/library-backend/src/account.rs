//! One account lifecycle for native hosts and browser workers.
mod client;
mod model;
mod network;
mod notifications;
mod storage;
use crate::executor::{sleep, timeout, BackendExecutor, BackendFuture, BackendOutput, BackendSend, BackendTask};
use crate::integration::{LibraryAccount, LibraryAccountSession};
use crate::management::{LibraryService, LibraryServiceConfig};
use crate::BackendError;
pub use client::{Client, Command, Transport};
use client_platform_runtime::connectivity::Connectivity;
use client_runtime::Failure;
pub use model::{AccountSnapshot, AccountStatus, ServerBookStorageUsage, ServerLibraryStorageUsage};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;
pub use storage::SessionStorage;
use tokio::sync::watch;

pub type AccountSession = LibraryAccountSession<SessionStorage>;

#[derive(Clone)]
pub struct Service {
    state: Arc<State>,
    task: Arc<Mutex<Option<BackendTask<()>>>>,
}
struct State {
    account: AccountSession,
    executor: BackendExecutor,
    libraries: LibraryService,
}

impl Service {
    pub fn new(account: AccountSession, libraries: LibraryService, executor: BackendExecutor, requests: watch::Receiver<()>) -> Self {
        let task = start(&executor, &libraries, account.library_account(), requests);
        Self { state: Arc::new(State { account, libraries, executor }), task: Arc::new(Mutex::new(Some(task))) }
    }
    pub fn shutdown(&self) {
        self.task.lock().unwrap_or_else(PoisonError::into_inner).take();
    }
    fn with_storage<T: BackendOutput>(&self, operation: impl FnOnce(&Self) -> Result<T, BackendError> + BackendSend + 'static) -> Result<T, BackendError> {
        let service = self.clone();
        self.state.libraries.with_storage(move |_| operation(&service))
    }
    pub async fn dispatch(&self, command: Command) -> Result<AccountSnapshot, BackendError> {
        // Reconciliation waits for storage and library owners. Both hosts run
        // it here so foreground callers never block on those synchronous waits.
        let service = self.clone();
        self.state.executor.spawn(async move { service.execute(command).await }).await.map_err(BackendError::operation)?
    }
    async fn execute(&self, command: Command) -> Result<AccountSnapshot, BackendError> {
        match command {
            Command::Snapshot => {}
            Command::Login { email, password } => self.login(&email, &password).await?,
            Command::VerifyEmail { email, pin, password } => self.verify_email(&email, &pin, &password).await?,
            Command::Logout => {
                let service = self.clone();
                self.state.libraries.run(move |_| service.logout()).await?;
            }
            Command::DeleteAccount { email, password } => self.delete_account(&email, &password).await?,
        }
        self.account_snapshot().await
    }
    pub fn account_status(&self) -> Result<AccountStatus, BackendError> {
        let mut account = self.current_account()?;
        if account.is_none() {
            account = self.state.account.restore_if_empty().map_err(account_error)?;
        }
        Ok(account.as_ref().map(|session| AccountStatus::authenticated(session.server_url().as_str(), session.email())).unwrap_or_else(AccountStatus::signed_out))
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<(), BackendError> {
        let session = account_client::login(self.state.account.server_url(), email, password).await.map_err(auth_error)?;
        let service = self.clone();
        self.state.libraries.run(move |_| service.adopt_session(session)).await
    }

    pub fn adopt_session(&self, session: account_client::Session) -> Result<(), BackendError> {
        self.with_storage(move |this| {
            let account = session_identity(&session);
            this.state.libraries.adopt_account(&account, || this.state.account.set(session).map_err(account_error))
        })
    }

    pub async fn verify_email(&self, email: &str, pin: &str, password: &str) -> Result<(), BackendError> {
        let session = account_client::verify_email(self.state.account.server_url(), email, pin, password).await.map_err(auth_error)?;
        let service = self.clone();
        self.state.libraries.run(move |_| service.adopt_session(session)).await
    }

    pub fn logout(&self) -> Result<(), BackendError> {
        self.logout_with_notification(|session| async move {
            match timeout(Duration::from_secs(8), account_client::logout(&session)).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => log::warn!("Failed to notify server about logout: {error}"),
                Err(_) => log::warn!("Server logout notification timed out"),
            }
        })
    }

    pub async fn delete_account(&self, email: &str, password: &str) -> Result<(), BackendError> {
        let session = self.state.account.require().map_err(account_error)?;
        if session.email() != email {
            return Err(BackendError::classified(Failure::AccountChanged, "the signed-in account is not the one being deleted"));
        }
        account_client::delete_account(&session, password).await.map_err(auth_error)?;
        let account = self.state.account.clone();
        self.state
            .libraries
            .run(move |libraries| {
                account.clear_user(session.user_id()).map_err(account_error)?;
                libraries.reconcile_account_libraries()
            })
            .await
    }

    pub fn logout_with_notification<F: BackendFuture<()>>(&self, notify: impl FnOnce(account_client::Session) -> F) -> Result<(), BackendError> {
        let session = self.with_storage(move |this| {
            this.state.libraries.retire_other_account_libraries(None)?;
            let session = this.state.account.take().map_err(account_error)?;
            this.state.libraries.reconcile_account_libraries()?;
            Ok(session)
        })?;
        if let Some(session) = session {
            // Revoke only the captured session. A subsequent login must not
            // wait for or be affected by this best-effort notification.
            self.state.executor.spawn_detached(notify(session));
        }
        Ok(())
    }

    pub async fn account_snapshot(&self) -> Result<AccountSnapshot, BackendError> {
        let started = web_time::Instant::now();
        let service = self.clone();
        let account = self.state.libraries.run(move |_| service.account_status()).await?;
        let (state, storage_usage, server_storage_usage, server_book_storage_usage) = if self.current_account()?.is_some() {
            let remote = self.state.libraries.refresh_remote_account_details().await?;
            (remote.state, Some(remote.storage_usage), Some(remote.server_storage_usage), remote.server_book_storage_usage)
        } else {
            (self.state.libraries.library_list_state(None, None).await?, None, None, None)
        };
        log::debug!("account snapshot took {:?}", started.elapsed());
        let pending_cloud_bytes = if account.signed_in() { self.state.libraries.pending_cloud_books().await.ok().and_then(|books| books.values().try_fold(0_u64, |bytes, book| bytes.checked_add(*book))) } else { None };
        Ok(AccountSnapshot { account, storage_usage, pending_cloud_bytes, sync_statuses: state.sync_statuses, server_storage_usage, server_book_storage_usage })
    }

    pub fn current_account(&self) -> Result<Option<account_client::Session>, BackendError> {
        self.state.account.current().map_err(account_error)
    }
}

const MAINTENANCE_RETRY_INTERVAL: Duration = Duration::from_secs(30);
const MAINTENANCE_DEBOUNCE: Duration = Duration::from_millis(100);

fn session_identity(session: &account_client::Session) -> String {
    format!("{}|{}", session.server_url(), session.user_id())
}

/// An account request's error, with what the account client knows about it.
pub fn auth_error(error: account_client::AuthError) -> BackendError {
    match error {
        account_client::AuthError::SignInRequired(reason) => BackendError::classified(Failure::SignInRequired, reason),
        account_client::AuthError::PasswordTooLong => BackendError::classified(Failure::PasswordTooLong, "password exceeds the maximum length"),
        account_client::AuthError::Http(error) if error.status() == Some(reqwest::StatusCode::UPGRADE_REQUIRED) => BackendError::classified(Failure::ClientUpgradeRequired, error.to_string()),
        account_client::AuthError::Wire(sync_common::transport::WireError::UnsupportedVersion(version)) => BackendError::classified(Failure::ClientUpgradeRequired, format!("unsupported account response protocol version {version}")),
        error => BackendError::operation(error),
    }
}

/// An account session's error, with what the account client knows about it.
pub fn account_error(error: account_client::AccountSessionError) -> BackendError {
    match error {
        account_client::AccountSessionError::Authentication(error) => auth_error(error),
        account_client::AccountSessionError::Changed => BackendError::classified(Failure::AccountChanged, "account changed while the request was in progress"),
        error => BackendError::operation(error),
    }
}

fn start(executor: &BackendExecutor, libraries: &LibraryService, account: LibraryAccount, requests: watch::Receiver<()>) -> BackendTask<()> {
    let state = Arc::downgrade(&libraries.state);
    let library_state = state.clone();
    let mut changes = account.subscribe();
    changes.borrow_and_update();
    let library_changes = changes.clone();
    let wake = libraries.state.account_wake.clone();
    let interest = libraries.state.notification_interest.clone();
    let directory = libraries.state.library_directory.clone();
    executor.spawn(async move {
        tokio::join!(notifications::run(account, wake, interest, directory), run_maintenance(state, requests, changes), reconcile_account_changes(library_state, library_changes));
    })
}

async fn reconcile_account_changes(state: Weak<LibraryServiceConfig>, mut changes: watch::Receiver<u64>) {
    while changes.changed().await.is_ok() {
        let Some(state) = state.upgrade() else { return };
        if let Err(error) = (LibraryService { state }).reconcile_account_libraries() {
            log::warn!("could not update libraries after account change: {error}");
        }
    }
}

async fn run_maintenance(state: Weak<LibraryServiceConfig>, requests: watch::Receiver<()>, changes: watch::Receiver<u64>) {
    let network = match network::observe() {
        Ok(network) => network,
        Err(error) => {
            log::warn!("Could not observe account connectivity: {error}");
            return;
        }
    };
    run_maintenance_with_network(state, requests, changes, network).await;
}

async fn run_maintenance_with_network(state: Weak<LibraryServiceConfig>, mut requests: watch::Receiver<()>, mut changes: watch::Receiver<u64>, mut network: Connectivity) {
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
        let backend = LibraryService { state };
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
