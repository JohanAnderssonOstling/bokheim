//! Library discovery, queued changes, and account access policy.
use super::LibraryService;
use crate::account::{ServerBookStorageUsage, ServerLibraryStorageUsage, SessionStorage, account_error};
use crate::{BackendError, LibraryHandle, LibraryId, LibrarySyncState, LibrarySyncStatus};
use account_client::{AuthError, PendingSessionChange, Session, SessionChange};
#[cfg(not(target_arch = "wasm32"))]
use client_platform_native::reserve_database_capacity;
use client_platform_runtime::storage_queue;
#[cfg(target_arch = "wasm32")]
use client_platform_web::reserve_database_capacity;
use futures_util::{StreamExt, stream};
use library_registry::{LibraryConfiguration, LibraryEntry};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;
use sync_common::api::libraries::LibraryCloudStorage;

#[derive(Clone, Debug, serde::Deserialize, Serialize)]
pub struct LibraryListState {
    pub libraries: Vec<LibraryEntry>,
    pub sync_statuses: Vec<LibrarySyncStatus>,
}

pub(crate) struct RemoteAccountDetails {
    pub(crate) state: LibraryListState,
    pub(crate) storage_usage: account_contract::StorageUsageResponse,
    pub(crate) server_storage_usage: Vec<ServerLibraryStorageUsage>,
    pub(crate) server_book_storage_usage: Option<Vec<ServerBookStorageUsage>>,
}

impl LibraryService {
    pub async fn finish_local_purge(self: &Arc<Self>, library_id: LibraryId) -> Result<(), BackendError> {
        if self.try_handle(&library_id).is_some() {
            return Err(BackendError::message("library is still open during local cleanup"));
        }
        #[cfg(target_arch = "wasm32")]
        client_platform_web::web_storage::purge_library(library_id).await.map_err(BackendError::message)?;
        self.config.registry.access(move |registry| registry.complete_local_purge(&library_id)).await.map_err(BackendError::operation)
    }

    pub async fn library_asset_storage_enabled(self: &Arc<Self>, id: &LibraryId) -> Result<bool, BackendError> {
        let id = *id;
        self.run(move |service| service.library_asset_storage_enabled_on_storage(&id)).await
    }

    pub(crate) fn library_asset_storage_enabled_on_storage(&self, id: &LibraryId) -> Result<bool, BackendError> {
        self.config.registry.asset_storage_enabled(id).map_err(BackendError::operation)
    }
    pub(crate) async fn require_account(self: &Arc<Self>) -> Result<account_client::Session, BackendError> {
        self.run(move |service| service.config.account.require().map_err(account_error)).await
    }
    pub fn send_library_list_changed(&self) {
        self.config.library_list_changed.send_replace(());
    }
    pub(crate) async fn apply_account_change(self: &Arc<Self>, change: PendingSessionChange<SessionStorage>) -> Result<Option<Session>, BackendError> {
        let result = self
            .run(move |this| {
                let identity = match change.change() {
                    SessionChange::SignIn(session) => {
                        if let Some(account) = this.account_identity_on_storage()? {
                            this.config.registry.bind_unowned_libraries(&account).map_err(BackendError::operation)?;
                        }
                        let identity = session.identity();
                        this.retire_other_account_libraries_on_storage(Some(&identity))?;
                        Some(identity)
                    }
                    SessionChange::SignOut => {
                        this.retire_other_account_libraries_on_storage(None)?;
                        None
                    }
                    SessionChange::AccountDeleted { .. } => None,
                };
                // Close inaccessible libraries before publishing new credentials.
                // A failed commit restores access using the still-current account.
                let session = match change.commit().map_err(account_error) {
                    Ok(session) => session,
                    Err(error) => return Err(error),
                };
                if let Some(identity) = identity {
                    this.config.registry.bind_unowned_libraries(&identity).map_err(BackendError::operation)?;
                }
                Ok(session)
            })
            .await;
        self.reconcile_account_libraries().await?;
        result
    }

    pub(crate) async fn refresh_remote_account_details(self: &Arc<Self>) -> Result<RemoteAccountDetails, BackendError> {
        let account = self.account_identity().await?;
        let expected_account = account.clone();
        let (snapshot, policies, book_storage) = tokio::try_join!(
            async {
                let started = web_time::Instant::now();
                let result = self.config.account.request(|session| Box::pin(async move { account_client::account_snapshot(&session).await })).await.map_err(account_error);
                log::debug!("account snapshot request took {:?}", started.elapsed());
                result
            },
            async {
                let started = web_time::Instant::now();
                let result = self.config.account.request(|session| Box::pin(async move { account_client::cloud_storage(&session).await })).await.map_err(account_error);
                log::debug!("account cloud storage request took {:?}", started.elapsed());
                result
            },
            async {
                let result = self
                    .config
                    .account
                    .request(|session| {
                        let authorized = expected_account.as_deref() == Some(session.identity().as_str());
                        Box::pin(async move {
                            if !authorized {
                                return Err(account_client::AuthError::SignInRequired("the account changed while loading book storage"));
                            }
                            account_client::book_storage_usage(&session).await
                        })
                    })
                    .await;
                result.map_err(account_error)
            },
        )?;
        let discovered = snapshot.libraries.clone();
        let discovery_account = account.clone();
        self.run(move |this| {
            this.check_account_identity_on_storage(&discovery_account)?;
            // This also identifies pending deletions from older installations,
            // which are excluded from the normal attachment below.
            for library in discovered {
                let already_bound = this.config.registry.accessible_to_account(&library.library_id, discovery_account.as_deref()).map_err(BackendError::operation)?;
                this.bind_to_current_account_on_storage(&library.library_id)?;
                if !already_bound {
                    if let Some(handle) = this.try_handle(&library.library_id) {
                        handle.request_remote_refresh();
                    }
                    this.config.account_wake.send_replace(());
                }
            }
            Ok(())
        })
        .await?;
        self.apply_remote_library_deletions(&snapshot.deleted_library_ids, &account).await?;
        let session = self.require_account().await?;
        // Settings is read-only and must not flush queued deletions. Keep those
        // libraries detached until the synchronization worker publishes the
        // deletion instead of resurrecting them from the remote snapshot.
        let pending_deletions = self.config.registry.access(move |registry| registry.pending_server_deletions()).await.map_err(BackendError::operation)?.into_iter().collect::<HashSet<_>>();
        let libraries = snapshot.libraries.into_iter().filter(|library| !pending_deletions.contains(&library.library_id)).collect::<Vec<_>>();
        reserve_database_capacity(libraries.len()).await.map_err(BackendError::message)?;
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
        self.check_account_identity(&account).await?;
        let remote_ids = self.attach_remote_libraries(&libraries).await?;
        self.check_account_identity(&account).await?;
        self.apply_cloud_storage(&policies).await?;
        let server_book_storage_usage = Some(self.resolve_server_book_storage(book_storage).await?);
        self.check_account_identity(&expected_account).await?;
        Ok(RemoteAccountDetails { state: self.library_list_state(Some(&session), Some(&remote_ids)).await?, storage_usage, server_storage_usage: library_storage_usage, server_book_storage_usage })
    }

    pub async fn resolve_server_book_storage(self: &Arc<Self>, books: Vec<account_contract::BookStorageUsage>) -> Result<Vec<ServerBookStorageUsage>, BackendError> {
        let hashes = books.iter().map(|book| book.book_id).collect::<Vec<_>>();
        let mut titles = std::collections::HashMap::new();
        if !hashes.is_empty() {
            for entry in self.list().await? {
                let Some(library) = self.try_handle(entry.library_id()) else { continue };
                match library.book_storage_titles(hashes.clone()).await {
                    Ok(reply) => {
                        for (hash, title) in reply {
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
        let mut books = books.into_iter().map(|book| ServerBookStorageUsage { book_id: book.book_id, title: titles.remove(&book.book_id).unwrap_or_else(|| "Title unavailable".into()), used_bytes: book.used_bytes }).collect::<Vec<_>>();
        books.sort_by(|a, b| b.used_bytes.cmp(&a.used_bytes).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())).then_with(|| a.book_id.cmp(&b.book_id)));
        Ok(books)
    }

    pub(crate) async fn flush_pending_library_deletions(self: &Arc<Self>) -> Result<bool, BackendError> {
        let mut all_flushed = true;
        for library_id in self.config.registry.access(move |registry| registry.pending_server_deletions()).await.map_err(BackendError::operation)? {
            if self.config.registry.access(move |registry| registry.device_only(&library_id)).await.map_err(BackendError::operation)? {
                continue;
            }
            if !{
                let account = self.account_identity().await?;
                self.config.registry.access(move |registry| registry.accessible_to_account(&library_id, account.as_deref())).await
            }
            .map_err(BackendError::operation)?
            {
                continue;
            }
            match self
                .config
                .account
                .request(|session| {
                    Box::pin(async move {
                        self.check_library_account(&library_id, &session).await?;
                        match account_client::delete_library(&session, &library_id).await.map(|_| ()) {
                            Err(error) if error.is_not_found() => Ok(()),
                            result => result,
                        }
                    })
                })
                .await
            {
                Ok(_) => {
                    if let Err(error) = self.config.registry.access(move |registry| registry.complete_server_deletion(&library_id)).await {
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

    pub(crate) async fn apply_remote_library_deletions(self: &Arc<Self>, deleted: &[LibraryId], account: &Option<String>) -> Result<(), BackendError> {
        let local_ids = self.list().await?.into_iter().map(|entry| *entry.library_id()).collect::<HashSet<_>>();
        for library_id in deleted.iter().filter(|library_id| local_ids.contains(library_id)) {
            if {
                let id = *library_id;
                self.config.registry.access(move |registry| registry.device_only(&id)).await
            }
            .map_err(BackendError::operation)?
            {
                continue;
            }
            let id = *library_id;
            let account = account.clone();
            self.run(move |this| {
                this.check_account_identity_on_storage(&account)?;
                this.config.registry.begin_local_removal(&id).map_err(BackendError::operation)?;
                this.retire(&id);
                this.config.registry.complete_server_deletion(&id).map_err(BackendError::operation)
            })
            .await?;
            self.send_library_list_changed();
            self.finish_local_purge(id).await?;
        }
        Ok(())
    }

    pub async fn attach_remote_libraries(self: &Arc<Self>, remote: &[sync_common::api::libraries::LibrarySummary]) -> Result<HashSet<LibraryId>, BackendError> {
        let account = self.account_identity().await?;
        let expected_account = account.clone();
        let _storage_change = self.config.storage_copy_lock.lock().await;
        let remote = remote.to_vec();
        let (remote_ids, needing_open, changed, updates, registered) = self
            .run(move |this| {
                this.check_account_identity_on_storage(&expected_account)?;
                let mut registered = Vec::new();
                let mut remote_ids = HashSet::with_capacity(remote.len());
                let mut needing_open = Vec::new();
                let mut changed = false;
                let mut updates = Vec::new();
                for library in &remote {
                    if this.config.registry.device_only(&library.library_id).map_err(BackendError::operation)? {
                        remote_ids.insert(library.library_id);
                        continue;
                    }
                    this.bind_to_current_account_on_storage(&library.library_id)?;
                    let existing = this.config.registry.entry(&library.library_id).map_err(BackendError::operation)?;
                    remote_ids.insert(library.library_id);
                    if this.config.registry.is_detached(&library.library_id).map_err(BackendError::operation)? {
                        continue;
                    }
                    registered.push(library.library_id);
                    // A stale server snapshot must not undo an offline rename.
                    if this.config.registry.pending_library_name(&library.library_id).map_err(BackendError::operation)?.is_some() {
                        continue;
                    }
                    let renamed = existing.as_ref().map(|entry| entry.library_name()) != Some(library.library_name.as_str());
                    if renamed {
                        changed = true;
                    }
                    // Library-list reconciliation updates registry metadata only. A live
                    // Library owns expensive shared resources (pool, scanner, sync manager,
                    // event forwarding) and must survive metadata refreshes unchanged.
                    let library_root = if existing.is_none() { this.effective_library_root()? } else { this.config.registry.default_library_root().to_owned() };
                    this.config.registry.attach_remote(&library.library_id, &library.library_name, &library_root).map_err(BackendError::operation)?;
                    let previous = this.config.registry.storage(&library.library_id).map_err(BackendError::operation)?;
                    this.config.registry.cache_remote_storage(&library.library_id, library.storage.clone()).map_err(BackendError::operation)?;
                    let current = this.config.registry.storage(&library.library_id).map_err(BackendError::operation)?;
                    if let Some(handle) = this.try_handle(&library.library_id) {
                        if previous != current {
                            updates.push((handle, current.uses_bokheim_assets()));
                        }
                    } else {
                        needing_open.push(library.library_id);
                    }
                }
                Ok((remote_ids, needing_open, changed, updates, registered))
            })
            .await?;
        for (handle, enabled) in updates {
            if let Err(error) = handle.apply_storage_policy(enabled, true).await {
                log::warn!("could not update library storage policy: {error}");
            }
        }
        drop(_storage_change);
        // Each open rechecks registry policy in its own storage command. A
        // failed open leaves the library listed but unavailable.
        for library_id in needing_open {
            if let Err(error) = self.open(&library_id).await {
                log::warn!("Could not open registered library {library_id} during discovery: {error}");
            }
        }
        self.check_account_identity(&account).await?;
        if let Some(identity) = account {
            for id in registered {
                if let Some(handle) = self.try_handle(&id) {
                    handle.mark_remote_registered(identity.clone()).await?;
                }
            }
        }
        if changed {
            self.send_library_list_changed();
        }
        Ok(remote_ids)
    }

    pub async fn library_list_state(self: &Arc<Self>, session: Option<&account_client::Session>, remote: Option<&HashSet<LibraryId>>) -> Result<LibraryListState, BackendError> {
        let libraries = self.list().await?;
        let started = web_time::Instant::now();
        let account = session.map(Session::identity);
        let ids = libraries.iter().map(|library| *library.library_id()).collect::<Vec<_>>();
        let configurations = self.config.registry.access(move |registry| ids.into_iter().map(|id| Ok((id, registry.configuration(&id)?))).collect::<library_registry::RegistryResult<Vec<_>>>()).await.map_err(BackendError::operation)?;
        let requests = configurations.into_iter().map(|(id, configuration)| library_sync_status(id, self.try_handle(&id), configuration, account.as_deref(), remote.map(|remote| remote.contains(&id))));
        let sync_statuses = stream::iter(requests).buffered(4).collect().await;
        log::debug!("account library statuses took {:?}", started.elapsed());
        Ok(LibraryListState { libraries, sync_statuses })
    }

    pub(crate) async fn publish_pending_library_names(self: &Arc<Self>) -> Result<(), BackendError> {
        if self.current_account().await?.is_none() {
            return Ok(());
        }
        let mut failure = None;
        for entry in self.list().await? {
            let id = *entry.library_id();
            if self.config.registry.access(move |registry| registry.device_only(&id)).await.map_err(BackendError::operation)? {
                continue;
            }
            if !{
                let account = self.account_identity().await?;
                self.config.registry.access(move |registry| registry.accessible_to_account(&id, account.as_deref())).await
            }
            .map_err(BackendError::operation)?
            {
                continue;
            }
            let result = async {
                let Some(name) = self.config.registry.access(move |registry| registry.pending_library_name(&id)).await.map_err(BackendError::operation)? else {
                    return Ok::<(), BackendError>(());
                };
                self.config
                    .account
                    .request(|session| {
                        let name = name.clone();
                        Box::pin(async move {
                            self.check_library_account(&id, &session).await?;
                            account_client::rename_library(&session, id, name).await
                        })
                    })
                    .await
                    .map_err(account_error)?;
                self.run(move |this| {
                    // A second rename during the request must remain queued.
                    this.config.registry.acknowledge_library_name(&id, &name).map_err(BackendError::operation)
                })
                .await
            }
            .await;
            if let Err(error) = result {
                log::warn!("rename for library {id} remains queued: {error}");
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub(crate) async fn run_account_maintenance(self: &Arc<Self>) -> Result<(), ()> {
        let result = async {
            let account = self.account_identity().await?;
            if account.is_none() {
                return Ok(());
            }
            let (writes, refresh) = tokio::join!(self.flush_account_writes_cycle(), self.refresh_remote_account_details());
            writes?;
            refresh?;
            self.check_account_identity(&account).await
        }
        .await;
        result.map_err(|error| log::warn!("account discovery will retry: {error}"))
    }

    pub(crate) async fn flush_account_writes_cycle(self: &Arc<Self>) -> Result<(), BackendError> {
        let mut changed = self.config.account.subscribe();
        changed.borrow_and_update();
        let work = async {
            if self.current_account().await?.is_none() {
                return Ok(());
            }
            // Register first so pending renames and storage changes have a
            // server library to update. Other writes still get an attempt if
            // registration for one library fails.
            let registrations = self.publish_pending_library_registrations().await;
            let (names, deletions, cloud) = tokio::join!(self.publish_pending_library_names(), self.flush_pending_library_deletions(), self.publish_pending_cloud_storage());
            registrations?;
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

    pub(crate) fn retire_other_account_libraries_on_storage(&self, account: Option<&str>) -> Result<(), BackendError> {
        for library in self.open_handles() {
            if self.config.registry.entry(library.id()).map_err(BackendError::operation)?.is_none() {
                continue;
            }
            if !self.config.registry.accessible_to_account(library.id(), account).map_err(BackendError::operation)? {
                self.retire(library.id());
            }
        }
        Ok(())
    }

    pub(crate) async fn reconcile_account_libraries(self: &Arc<Self>) -> Result<(), BackendError> {
        self.run(move |service| service.retire_other_account_libraries_on_storage(service.account_identity_on_storage()?.as_deref())).await?;
        self.open_registered().await?;
        self.send_library_list_changed();
        Ok(())
    }

    pub(crate) async fn check_library_account(self: &Arc<Self>, id: &LibraryId, session: &Session) -> Result<(), AuthError> {
        let id = *id;
        let session = session.clone();
        let registry = self.config.registry.clone();
        storage_queue::run(move || {
            if registry.device_only(&id).map_err(|error| AuthError::InvalidData(error.to_string()))? {
                return Err(AuthError::SignInRequired("this library is kept only on this device"));
            }
            if registry.accessible_to_account(&id, Some(&session.identity())).map_err(|error| AuthError::InvalidData(error.to_string()))? { Ok(()) } else { Err(AuthError::SignInRequired("sign in to the account that owns this library")) }
        })
        .await
        .map_err(AuthError::InvalidData)?
    }

    pub async fn check_account_identity(self: &Arc<Self>, account: &Option<String>) -> Result<(), BackendError> {
        let account = account.clone();
        self.run(move |service| service.check_account_identity_on_storage(&account)).await
    }

    pub(crate) fn check_account_identity_on_storage(&self, account: &Option<String>) -> Result<(), BackendError> {
        if self.account_identity_on_storage()? != *account {
            return Err(BackendError::message("account changed during background synchronization"));
        }
        Ok(())
    }

    pub(crate) async fn publish_cloud_storage(self: &Arc<Self>, id: LibraryId) -> Result<(), BackendError> {
        if self.config.registry.access(move |registry| registry.device_only(&id)).await.map_err(BackendError::operation)? {
            return Ok(());
        }
        if !{
            let account = self.account_identity().await?;
            self.config.registry.access(move |registry| registry.accessible_to_account(&id, account.as_deref())).await
        }
        .map_err(BackendError::operation)?
        {
            return Ok(());
        }
        let Some(pending) = self.config.registry.access(move |registry| registry.pending_cloud_storage(&id)).await.map_err(BackendError::operation)? else { return Ok(()) };
        let account = self.account_identity().await?;
        let result = self
            .config
            .account
            .request(|session| {
                let change = pending.change.clone();
                Box::pin(async move {
                    self.check_library_account(&id, &session).await?;
                    account_client::set_cloud_storage(&session, id, change).await
                })
            })
            .await
            .map_err(account_error)?;
        let _storage_change = self.config.storage_copy_lock.lock().await;
        let enabled = result.enabled;
        let library = self
            .run(move |this| {
                this.check_account_identity_on_storage(&account)?;
                let acknowledged = this.config.registry.acknowledge_cloud_storage(&id, pending, enabled).map_err(BackendError::operation)?;
                this.config.account_wake.send_replace(());
                Ok(acknowledged.then(|| this.try_handle(&id)).flatten())
            })
            .await?;
        if let Some(library) = library {
            library.apply_storage_policy(enabled, true).await?;
        }
        Ok(())
    }

    pub(crate) async fn apply_cloud_storage(self: &Arc<Self>, states: &[LibraryCloudStorage]) -> Result<(), BackendError> {
        let _storage_change = self.config.storage_copy_lock.lock().await;
        let states = states.to_vec();
        let updates = self
            .run(move |this| {
                let mut updates = Vec::new();
                for state in &states {
                    let id = state.library_id;
                    if this.config.registry.entry(&id).map_err(BackendError::operation)?.is_none()
                        || this.config.registry.device_only(&id).map_err(BackendError::operation)?
                        || this.config.registry.pending_cloud_storage(&id).map_err(BackendError::operation)?.is_some()
                    {
                        continue;
                    }
                    if this.library_asset_storage_enabled_on_storage(&id)? != state.enabled {
                        this.config.registry.cache_cloud_storage_with_reset(&id, state.enabled).map_err(BackendError::operation)?;
                        if let Some(library) = this.try_handle(&id) {
                            updates.push((library, state.enabled));
                        }
                    }
                }
                Ok(updates)
            })
            .await?;
        for (library, enabled) in updates {
            if let Err(error) = library.apply_storage_policy(enabled, true).await {
                log::warn!("could not update library storage policy: {error}");
            }
        }
        Ok(())
    }

    pub(crate) async fn publish_pending_cloud_storage(self: &Arc<Self>) -> Result<(), BackendError> {
        let mut failure = None;
        for entry in self.list().await? {
            let id = *entry.library_id();
            if self.config.registry.access(move |registry| registry.pending_cloud_storage(&id)).await.map_err(BackendError::operation)?.is_none() {
                continue;
            }
            if let Err(error) = self.publish_cloud_storage(id).await {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

async fn library_sync_status(id: LibraryId, handle: Option<LibraryHandle>, configuration: LibraryConfiguration, account: Option<&str>, remote_present: Option<bool>) -> LibrarySyncStatus {
    let belongs_to_account = configuration.accessible_to_account(account);
    let device_only = configuration.device_only();
    if let Some(handle) = handle {
        match handle.sync_status(account.is_some() && belongs_to_account && !device_only, remote_present).await {
            Ok(mut status) => {
                status.storage = configuration.storage();
                status.asset_storage_enabled &= status.storage.uses_bokheim_assets();
                if account.is_some() && !belongs_to_account {
                    status.state = LibrarySyncState::AccountRequired;
                }
                return status;
            }
            Err(error) => log::warn!("Could not read sync status for library {}: {error}", id),
        }
    }
    let mut status = LibrarySyncStatus::placeholder(id, LibrarySyncState::Unavailable);
    status.storage = configuration.storage();
    status.asset_storage_enabled = status.storage.uses_bokheim_assets();
    status
}
