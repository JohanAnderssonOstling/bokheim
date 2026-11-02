//! Library discovery, queued changes, and account access policy.
use super::{session_identity, LibraryService};
use crate::account::{account_error, ServerBookStorageUsage, ServerLibraryStorageUsage};
use crate::library::handle::{LibraryActorCommand, LibraryControl};
use crate::{BackendError, BookId, LibraryCommand, LibraryId, LibrarySyncState, LibrarySyncStatus, ReadyCommand};
#[cfg(not(target_arch = "wasm32"))]
use client_platform_native::reserve_database_capacity;
#[cfg(target_arch = "wasm32")]
use client_platform_web::reserve_database_capacity;
use futures_util::{stream, StreamExt, TryStreamExt};
use library_registry::LibraryEntry;
use serde::Serialize;
use std::collections::HashSet;
use sync_common::api::libraries::{CloudStorageChange, LibraryCloudStorage};

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
    pub async fn finish_local_purge(&self, library_id: LibraryId) -> Result<(), BackendError> {
        if self.state.library_directory.try_get(&library_id)?.is_some() {
            return Err(BackendError::message("library is still open during local cleanup"));
        }
        #[cfg(target_arch = "wasm32")]
        client_platform_web::web_storage::purge_library(library_id).await.map_err(BackendError::message)?;
        self.state.registry.complete_local_purge(&library_id).map_err(BackendError::operation)
    }

    pub fn library_asset_storage_enabled(&self, id: &LibraryId) -> Result<bool, BackendError> {
        self.state.registry.asset_storage_enabled(id).map_err(BackendError::operation)
    }
    pub(crate) fn require_account(&self) -> Result<account_client::Session, BackendError> {
        self.state.account.require().map_err(account_error)
    }
    pub fn send_library_list_changed(&self) {
        self.state.library_list_changed.send_replace(());
    }
    pub(crate) fn adopt_account(&self, identity: &str, change: impl FnOnce() -> Result<(), BackendError>) -> Result<(), BackendError> {
        if let Some(account) = self.account_identity()? {
            self.state.registry.bind_unowned_libraries(&account).map_err(BackendError::operation)?;
        }
        self.retire_other_account_libraries(Some(identity))?;
        if let Err(error) = change() {
            self.reconcile_account_libraries()?;
            return Err(error);
        }
        self.state.registry.bind_unowned_libraries(identity).map_err(BackendError::operation)?;
        self.reconcile_account_libraries()
    }

    pub(crate) async fn refresh_remote_account_details(&self) -> Result<RemoteAccountDetails, BackendError> {
        let account = self.account_identity()?;
        let expected_account = account.clone();
        let (snapshot, policies, book_storage) = tokio::try_join!(
            async {
                let started = web_time::Instant::now();
                let result = self.state.account.request(|session| Box::pin(async move { account_client::account_snapshot(&session).await })).await.map_err(account_error);
                log::debug!("account snapshot request took {:?}", started.elapsed());
                result
            },
            async {
                let started = web_time::Instant::now();
                let result = self.state.account.request(|session| Box::pin(async move { account_client::cloud_storage(&session).await })).await.map_err(account_error);
                log::debug!("account cloud storage request took {:?}", started.elapsed());
                result
            },
            async {
                let result = self
                    .state
                    .account
                    .request(|session| {
                        let authorized = expected_account.as_deref() == Some(session_identity(&session).as_str());
                        Box::pin(async move {
                            if !authorized {
                                return Err(account_client::AuthError::SignInRequired("the account changed while loading book storage"));
                            }
                            account_client::book_storage_usage(&session).await
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
                this.bind_to_current_account(&library.library_id)?;
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

    pub async fn resolve_server_book_storage(&self, books: Vec<account_contract::BookStorageUsage>) -> Result<Vec<ServerBookStorageUsage>, BackendError> {
        let hashes = books.iter().map(|book| book.book_id).collect::<Vec<_>>();
        let mut titles = std::collections::HashMap::new();
        if !hashes.is_empty() {
            for entry in self.list()? {
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
        let mut books = books.into_iter().map(|book| ServerBookStorageUsage { book_id: book.book_id, title: titles.remove(&book.book_id).unwrap_or_else(|| "Title unavailable".into()), used_bytes: book.used_bytes }).collect::<Vec<_>>();
        books.sort_by(|a, b| b.used_bytes.cmp(&a.used_bytes).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())).then_with(|| a.book_id.cmp(&b.book_id)));
        Ok(books)
    }

    pub(crate) async fn flush_pending_library_deletions(&self) -> Result<bool, BackendError> {
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
                    let authorized = self.check_library_account(&library_id, &session);
                    Box::pin(async move {
                        authorized?;
                        account_client::delete_library(&session, &library_id).await
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

    pub(crate) async fn apply_remote_library_deletions(&self, deleted: &[LibraryId], account: &Option<String>) -> Result<(), BackendError> {
        let local_ids = self.list()?.into_iter().map(|entry| *entry.library_id()).collect::<HashSet<_>>();
        for library_id in deleted.iter().filter(|library_id| local_ids.contains(library_id)) {
            if self.state.registry.device_only(library_id).map_err(BackendError::operation)? {
                continue;
            }
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

    pub fn attach_remote_libraries(&self, remote: &[sync_common::api::libraries::LibrarySummary]) -> Result<HashSet<LibraryId>, BackendError> {
        let remote = remote.to_vec();
        let (remote_ids, needing_open, changed) = self.with_storage(move |this| {
            let mut remote_ids = HashSet::with_capacity(remote.len());
            let mut needing_open = Vec::new();
            let mut changed = false;
            for library in &remote {
                if this.state.registry.device_only(&library.library_id).map_err(BackendError::operation)? {
                    remote_ids.insert(library.library_id);
                    continue;
                }
                this.bind_to_current_account(&library.library_id)?;
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
                let kind_changed = existing.as_ref().is_some_and(|entry| entry.kind() != &library.kind);
                this.state.registry.set_kind(&library.library_id, library.kind.clone()).map_err(BackendError::operation)?;
                if kind_changed {
                    this.state.library_directory.retire(&library.library_id)?;
                    changed = true;
                }
                let previous = this.state.registry.storage(&library.library_id).map_err(BackendError::operation)?;
                this.state.registry.cache_remote_storage(&library.library_id, library.storage.clone()).map_err(BackendError::operation)?;
                let current = this.state.registry.storage(&library.library_id).map_err(BackendError::operation)?;
                if previous != current {
                    this.apply_open_cloud_storage(library.library_id, current.uses_bokheim_assets())?;
                }

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
            if let Err(error) = self.open(&library_id) {
                log::warn!("Could not open registered library {library_id} during discovery: {error}");
            }
        }
        if changed {
            self.send_library_list_changed();
        }
        Ok(remote_ids)
    }

    pub async fn library_list_state(&self, session: Option<&account_client::Session>, remote: Option<&HashSet<LibraryId>>) -> Result<LibraryListState, BackendError> {
        let libraries = self.list()?;
        let started = web_time::Instant::now();
        let sync_statuses = stream::iter(libraries.iter().cloned().map(|library| async move { self.library_sync_status(&library, session, remote).await })).buffered(4).try_collect().await?;
        log::debug!("account library statuses took {:?}", started.elapsed());
        Ok(LibraryListState { libraries, sync_statuses })
    }

    pub async fn library_sync_status(&self, library: &LibraryEntry, session: Option<&account_client::Session>, remote: Option<&HashSet<LibraryId>>) -> Result<LibrarySyncStatus, BackendError> {
        let account = session.map(session_identity);
        let configuration = self.state.registry.configuration(library.library_id()).map_err(BackendError::operation)?;
        let belongs_to_account = configuration.accessible_to_account(account.as_deref());
        let device_only = configuration.device_only();
        if let Some(handle) = self.state.library_directory.try_get(library.library_id())? {
            match handle
                .dispatch(LibraryCommand::Ready(ReadyCommand::SyncStatus { signed_in: session.is_some() && belongs_to_account && !device_only, remote_present: remote.map(|remote| remote.contains(library.library_id())) }))
                .await
                .and_then(client_runtime::reply::Reply::take::<LibrarySyncStatus>)
            {
                Ok(mut status) => {
                    status.storage = configuration.storage();
                    status.asset_storage_enabled = status.storage.uses_bokheim_assets();
                    if session.is_some() && !belongs_to_account {
                        status.state = LibrarySyncState::AccountRequired;
                    }
                    return Ok(status);
                }
                Err(error) => log::warn!("Could not read sync status for library {}: {error}", library.library_id()),
            }
        }
        let mut status = LibrarySyncStatus::placeholder(*library.library_id(), LibrarySyncState::Unavailable);
        status.storage = configuration.storage();
        status.asset_storage_enabled = status.storage.uses_bokheim_assets();
        Ok(status)
    }

    pub(crate) async fn publish_pending_library_names(&self) -> Result<(), BackendError> {
        if self.current_account()?.is_none() {
            return Ok(());
        }
        let mut failure = None;
        for entry in self.list()? {
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
                        let authorized = self.check_library_account(&id, &session);
                        let name = name.clone();
                        Box::pin(async move {
                            authorized?;
                            account_client::rename_library(&session, id, name).await
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

    pub(crate) async fn run_account_maintenance(&self) -> Result<(), ()> {
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

    pub(crate) async fn flush_account_writes_cycle(&self) -> Result<(), BackendError> {
        let mut changed = self.state.account.subscribe();
        changed.borrow_and_update();
        let work = async {
            if self.current_account()?.is_none() {
                return Ok(());
            }
            // Every queue gets an attempt even if another fails.
            let (names, deletions, cloud) = tokio::join!(self.publish_pending_library_names(), self.flush_pending_library_deletions(), self.publish_pending_cloud_storage());
            cloud?;
            names?;
            if deletions? {
                Ok(())
            } else {
                Err(BackendError::message("library deletions remain pending"))
            }
        };
        tokio::select! {
            biased;
            _ = changed.changed() => Ok(()),
            result = work => result,
        }
    }

    pub(crate) fn retire_other_account_libraries(&self, account: Option<&str>) -> Result<(), BackendError> {
        for library in self.state.library_directory.all()? {
            if self.state.registry.entry(&library.id()).map_err(BackendError::operation)?.is_none() {
                continue;
            }
            if !self.state.registry.accessible_to_account(&library.id(), account).map_err(BackendError::operation)? {
                self.state.library_directory.retire(&library.id())?;
            }
        }
        Ok(())
    }

    pub(crate) fn reconcile_account_libraries(&self) -> Result<(), BackendError> {
        self.with_storage(move |this| {
            this.retire_other_account_libraries(this.account_identity()?.as_deref())?;
            this.open_registered()?;
            this.send_library_list_changed();
            Ok(())
        })
    }

    pub(crate) fn check_library_account(&self, id: &LibraryId, session: &account_client::Session) -> Result<(), account_client::AuthError> {
        if self.state.registry.device_only(id).map_err(|error| account_client::AuthError::InvalidData(error.to_string()))? {
            return Err(account_client::AuthError::SignInRequired("this library is kept only on this device"));
        }
        if self.state.registry.accessible_to_account(id, Some(&session_identity(session))).map_err(|error| account_client::AuthError::InvalidData(error.to_string()))? {
            Ok(())
        } else {
            Err(account_client::AuthError::SignInRequired("sign in to the account that owns this library"))
        }
    }

    pub fn check_account_identity(&self, account: &Option<String>) -> Result<(), BackendError> {
        if self.account_identity()? != *account {
            return Err(BackendError::message("account changed during background synchronization"));
        }
        Ok(())
    }

    pub(crate) async fn publish_cloud_storage(&self, id: LibraryId) -> Result<(), BackendError> {
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
                let authorized = self.check_library_account(&id, &session);
                let change = change.clone();
                Box::pin(async move {
                    authorized?;
                    account_client::set_cloud_storage(&session, id, change).await
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

    pub(crate) fn apply_cloud_storage(&self, states: &[LibraryCloudStorage]) -> Result<(), BackendError> {
        let states = states.to_vec();
        self.with_storage(move |this| {
            for state in &states {
                let id = state.library_id;
                if this.state.registry.entry(&id).map_err(BackendError::operation)?.is_none()
                    || this.state.registry.device_only(&id).map_err(BackendError::operation)?
                    || this.state.registry.read_value(&format!("pending-cloud-storage:{id}")).map_err(BackendError::operation)?.is_some()
                {
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

    pub(crate) fn apply_open_cloud_storage(&self, id: LibraryId, enabled: bool) -> Result<(), BackendError> {
        if let Ok(library) = self.state.library_directory.get(&id) {
            let _ = library.commands.try_send(LibraryActorCommand::Control(LibraryControl::SetAssetStorage { enabled, reset_cloud_presence: true }));
        }
        Ok(())
    }

    pub(crate) async fn publish_pending_cloud_storage(&self) -> Result<(), BackendError> {
        let mut failure = None;
        for entry in self.list()? {
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
