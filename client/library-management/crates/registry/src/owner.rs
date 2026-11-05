//! Cloneable registry ownership. Short transactions run on the storage boundary
//! and return owned values, never a SQLite borrow.
use super::{DATABASE_FILENAME, LibraryConfiguration, LibraryEntry, LocalRegistry, RegistryResult, join_locator};
use client_platform_runtime::storage_queue;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use sync_common::LibraryId;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn cloned_clients_share_one_registry_until_last_drop() -> RegistryResult<()> {
        let root = tempfile::tempdir()?;
        let registry = LibraryRegistry::open(root.path().to_string_lossy())?;
        let retained = Arc::downgrade(&registry.0);
        let clients = (0..6)
            .map(|index| {
                let registry = registry.clone();
                thread::spawn(move || -> RegistryResult<()> {
                    let key = format!("client-{index}");
                    registry.write_value(&key, &[index])?;
                    assert_eq!(registry.read_value(&key)?, Some(vec![index]));
                    Ok(())
                })
            })
            .collect::<Vec<_>>();
        for client in clients {
            client.join().unwrap()?;
        }
        assert!(retained.upgrade().is_some());
        drop(registry);
        assert!(retained.upgrade().is_none());
        Ok(())
    }

    #[test]
    fn account_ownership_survives_restart_and_local_removal_without_hiding_libraries() -> RegistryResult<()> {
        let root = tempfile::tempdir()?;
        let registry = LibraryRegistry::open(root.path().to_string_lossy())?;
        let entry = registry.create("Books", None)?;
        let id = *entry.library_id();
        registry.bind_unowned_libraries("server|first")?;
        registry.bind_unowned_libraries("server|second")?;
        assert!(registry.accessible_to_account(&id, Some("server|first"))?);
        assert!(!registry.accessible_to_account(&id, Some("server|second"))?);
        assert!(!registry.accessible_to_account(&id, Some("other-server|first"))?);
        assert_eq!(registry.entries()?.len(), 1);
        assert!(registry.bind_library_account(&id, "server|second").is_err());
        drop(registry);

        let registry = LibraryRegistry::open(root.path().to_string_lossy())?;
        assert!(registry.accessible_to_account(&id, Some("server|first"))?);
        let reopened = registry.create_with_asset_storage("Books", Some(entry.storage_locator().to_owned()), false)?;
        assert_eq!(*reopened.library_id(), id);
        assert!(registry.has_account(&id)?, "registering an existing root must preserve its owner");
        assert!(!registry.accessible_to_account(&id, Some("server|second"))?);
        assert!(registry.asset_storage_enabled(&id)?, "registration must preserve the existing storage choice");
        assert!(registry.read_value(&format!("pending-cloud-storage:{id}"))?.is_none());
        assert!(registry.make_device_only(&id).is_err(), "turning off book storage must preserve account ownership");
        assert!(registry.has_account(&id)?);
        registry.begin_local_removal(&id)?;
        assert_eq!(registry.pending_server_deletions()?, [id]);
        assert!(!registry.accessible_to_account(&id, Some("server|second"))?, "pending deletion retains its original account");
        Ok(())
    }

    #[test]
    fn account_filter_keeps_signed_out_caches_and_local_only_libraries_after_restart() -> RegistryResult<()> {
        let root = tempfile::tempdir()?;
        let registry = LibraryRegistry::open(root.path().to_string_lossy())?;
        let cloud = registry.create("Cloud", None)?;
        registry.bind_library_account(cloud.library_id(), "server|owner")?;
        let local = registry.create_with_asset_storage("Local", None, false)?;
        registry.make_device_only(local.library_id())?;
        registry.bind_unowned_libraries("server|other")?;
        assert_eq!(registry.entries_for_account(None)?.len(), 2);
        assert_eq!(registry.entries_for_account(Some("server|owner"))?.len(), 2);
        assert_eq!(registry.entries_for_account(Some("server|other"))?.iter().map(|entry| *entry.library_id()).collect::<Vec<_>>(), [*local.library_id()]);
        assert!(!registry.has_account(local.library_id())?);
        assert!(registry.bind_library_account(local.library_id(), "server|other").is_err());
        drop(registry);
        let registry = LibraryRegistry::open(root.path().to_string_lossy())?;
        assert_eq!(registry.entries_for_account(None)?.len(), 2);
        assert_eq!(registry.entries_for_account(Some("server|other"))?.len(), 1);
        assert!(registry.device_only(local.library_id())?);
        Ok(())
    }
}

struct Handle {
    registry: Mutex<Option<LocalRegistry>>,
    #[cfg(not(target_arch = "wasm32"))]
    app_data_locator: String,
    default_library_root: String,
}

impl Drop for Handle {
    fn drop(&mut self) {
        let registry = self.registry.get_mut().unwrap_or_else(PoisonError::into_inner).take();
        let _ = storage_queue::dispatch_cleanup(move || drop(registry));
    }
}

#[derive(Clone)]
pub struct LibraryRegistry(Arc<Handle>);

impl std::fmt::Debug for LibraryRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("LibraryRegistry").field("default_library_root", &self.0.default_library_root).finish_non_exhaustive()
    }
}

impl LibraryRegistry {
    pub fn open(app_data_locator: impl Into<String>) -> RegistryResult<Self> {
        let app_data_locator = app_data_locator.into();
        let library_root = join_locator(&app_data_locator, "libraries");
        Self::open_with_library_root(app_data_locator, library_root)
    }

    pub fn open_with_library_root(app_data_locator: impl Into<String>, default_library_root: impl Into<String>) -> RegistryResult<Self> {
        let app_data_locator = app_data_locator.into();
        let default_library_root = default_library_root.into();
        storage_queue::run_sync(move || {
            let registry = LocalRegistry::open_with_library_root(app_data_locator.clone(), default_library_root.clone())?;
            Ok(Self(Arc::new(Handle {
                registry: Mutex::new(Some(registry)),
                #[cfg(not(target_arch = "wasm32"))]
                app_data_locator,
                default_library_root,
            })))
        })?
    }

    /// Actual registry filename in the selected platform storage namespace.
    pub fn database_locator(app_data_locator: &str) -> String {
        join_locator(app_data_locator, DATABASE_FILENAME)
    }

    /// Native databases belong to the device, independently of the content folder.
    pub fn library_database_locator(&self, id: &LibraryId, content_locator: &str) -> String {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = content_locator;
            join_locator(&self.0.app_data_locator, &format!("libraries/{id}/library.db"))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = id;
            content_locator.to_owned()
        }
    }

    pub(super) fn run<T: Send + 'static>(&self, operation: impl FnOnce(&mut LocalRegistry) -> RegistryResult<T> + Send + 'static) -> RegistryResult<T> {
        let handle = self.0.clone();
        storage_queue::run_sync(move || {
            let mut registry = handle.registry.lock().unwrap_or_else(PoisonError::into_inner);
            operation(registry.as_mut().ok_or("library registry is closed")?)
        })?
    }

    /// Run a short registry transaction on its storage owner.
    pub async fn access<T: Send + 'static>(&self, operation: impl FnOnce(&Self) -> RegistryResult<T> + Send + 'static) -> RegistryResult<T> {
        let registry = self.clone();
        storage_queue::run(move || operation(&registry)).await.map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { error.into() })?
    }

    pub fn entries(&self) -> RegistryResult<Vec<LibraryEntry>> {
        self.run(move |registry| registry.entries())
    }

    pub fn entries_for_account(&self, account: Option<&str>) -> RegistryResult<Vec<LibraryEntry>> {
        let account = account.map(str::to_owned);
        self.run(move |registry| {
            registry
                .entries()?
                .into_iter()
                .filter_map(|entry| match registry.accessible_to_account(entry.library_id(), account.as_deref()) {
                    Ok(true) => Some(Ok(entry)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                })
                .collect()
        })
    }

    pub fn configuration(&self, id: &LibraryId) -> RegistryResult<LibraryConfiguration> {
        let id = *id;
        self.run(move |registry| registry.configuration(&id))
    }

    pub fn configure_cloud_storage(&self, id: &LibraryId, enabled: bool, account: Option<&str>) -> RegistryResult<()> {
        let id = *id;
        let account = account.map(str::to_owned);
        self.run(move |registry| registry.configure_cloud_storage(&id, enabled, account.as_deref()))
    }

    pub fn device_only(&self, id: &LibraryId) -> RegistryResult<bool> {
        let id = *id;
        self.run(move |registry| registry.device_only(&id))
    }

    pub fn has_account(&self, id: &LibraryId) -> RegistryResult<bool> {
        let id = *id;
        self.run(move |registry| registry.has_account(&id))
    }

    pub fn make_device_only(&self, id: &LibraryId) -> RegistryResult<()> {
        let id = *id;
        self.run(move |registry| registry.make_device_only(&id))
    }

    pub fn accessible_to_account(&self, id: &LibraryId, account: Option<&str>) -> RegistryResult<bool> {
        let id = *id;
        let account = account.map(str::to_owned);
        self.run(move |registry| registry.accessible_to_account(&id, account.as_deref()))
    }

    pub fn bind_library_account(&self, id: &LibraryId, account: &str) -> RegistryResult<()> {
        let id = *id;
        let account = account.to_owned();
        self.run(move |registry| registry.bind_library_account(&id, &account))
    }

    pub fn bind_unowned_libraries(&self, account: &str) -> RegistryResult<()> {
        let account = account.to_owned();
        self.run(move |registry| registry.bind_unowned_libraries(&account))
    }

    pub fn entry(&self, id: &LibraryId) -> RegistryResult<Option<LibraryEntry>> {
        let id = *id;
        self.run(move |registry| registry.entry(&id))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn recover_library_root(&self, id: &LibraryId, library_root: &str) -> RegistryResult<super::LibraryRootRecovery> {
        let id = *id;
        let library_root = library_root.to_owned();
        self.run(move |registry| registry.recover_library_root(&id, &library_root))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn remember_library_root(&self, id: &LibraryId) -> RegistryResult<()> {
        let id = *id;
        self.run(move |registry| registry.remember_library_root(&id))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn validate_move_destination(&self, id: LibraryId, source: PathBuf, destination: PathBuf) -> RegistryResult<PathBuf> {
        self.run(move |registry| registry.validate_move_destination(&id, &source, &destination))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn commit_library_move(&self, id: LibraryId, source: PathBuf, destination: PathBuf, source_identity: String, cleanup: Vec<u8>) -> RegistryResult<LibraryEntry> {
        self.run(move |registry| registry.commit_library_move(&id, &source, &destination, &source_identity, &cleanup))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn retire_library_move_source(&self, id: LibraryId, source: PathBuf, source_identity: String) -> RegistryResult<()> {
        self.run(move |registry| registry.retire_library_move_source(&id, &source, &source_identity))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn library_move_cleanup_keys(&self) -> RegistryResult<Vec<String>> {
        self.run(|registry| Ok(registry.connection.prepare("SELECT key FROM app_value WHERE key GLOB 'library-cleanup:v1:*' ORDER BY key")?.query_map([], |row| row.get(0))?.collect::<Result<_, _>>()?))
    }

    pub fn asset_storage_enabled(&self, id: &LibraryId) -> RegistryResult<bool> {
        let id = *id;
        self.run(move |registry| registry.asset_storage_enabled(&id))
    }

    pub fn storage(&self, id: &LibraryId) -> RegistryResult<super::LibraryStorage> {
        let id = *id;
        self.run(move |registry| registry.storage(&id))
    }

    pub fn cache_remote_storage(&self, id: &LibraryId, storage: super::LibraryStorage) -> RegistryResult<()> {
        let id = *id;
        self.run(move |registry| registry.cache_remote_storage(&id, storage))
    }

    pub fn create_with_storage_root(&self, name: &str, locator: Option<String>, storage: super::LibraryStorage, library_root: &str) -> RegistryResult<LibraryEntry> {
        let name = name.to_owned();
        let library_root = library_root.to_owned();
        self.run(move |registry| registry.create_with_storage_root(&name, locator, storage, &library_root))
    }

    pub fn create_with_storage(&self, name: &str, locator: Option<String>, storage: super::LibraryStorage) -> RegistryResult<LibraryEntry> {
        let name = name.to_owned();
        self.run(move |registry| registry.create_with_storage(&name, locator, storage))
    }

    pub fn set_asset_storage_enabled(&self, id: &LibraryId, enabled: bool) -> RegistryResult<()> {
        let id = *id;
        self.run(move |registry| registry.set_asset_storage_enabled(&id, enabled))
    }

    pub fn cache_cloud_storage_with_reset(&self, id: &LibraryId, enabled: bool) -> RegistryResult<()> {
        let id = *id;
        self.run(move |registry| registry.cache_cloud_storage_with_reset(&id, enabled))
    }

    pub fn cloud_storage_reset(&self, id: &LibraryId) -> RegistryResult<Option<super::CloudStorageReset>> {
        let id = *id;
        self.run(move |registry| registry.cloud_storage_reset(&id))
    }

    pub fn acknowledge_cloud_storage_reset(&self, id: &LibraryId, reset: super::CloudStorageReset) -> RegistryResult<()> {
        let id = *id;
        self.run(move |registry| registry.acknowledge_cloud_storage_reset(&id, reset))
    }

    pub fn pending_library_name(&self, id: &LibraryId) -> RegistryResult<Option<String>> {
        let id = *id;
        self.run(move |registry| registry.pending_library_name(&id))
    }

    pub fn rename_library(&self, id: &LibraryId, name: &str) -> RegistryResult<LibraryEntry> {
        let id = *id;
        let name = name.to_owned();
        self.run(move |registry| registry.rename_library(&id, &name))
    }

    pub fn acknowledge_library_name(&self, id: &LibraryId, name: &str) -> RegistryResult<()> {
        let id = *id;
        let name = name.to_owned();
        self.run(move |registry| registry.acknowledge_library_name(&id, &name))
    }

    pub fn pending_cloud_storage(&self, id: &LibraryId) -> RegistryResult<Option<super::PendingCloudStorage>> {
        let id = *id;
        self.run(move |registry| registry.pending_cloud_storage(&id))
    }

    pub fn acknowledge_cloud_storage(&self, id: &LibraryId, pending: super::PendingCloudStorage, enabled: bool) -> RegistryResult<bool> {
        let id = *id;
        self.run(move |registry| registry.acknowledge_cloud_storage(&id, pending, enabled))
    }

    pub fn clear_value_if_current(&self, key: &str, value: &[u8]) -> RegistryResult<()> {
        let key = key.to_owned();
        let value = value.to_vec();
        self.run(move |registry| registry.clear_value_if_current(&key, &value))
    }

    /// Save the local cache and an idempotent account write together.
    pub fn queue_cloud_storage(&self, id: &LibraryId, enabled: bool) -> RegistryResult<()> {
        let id = *id;
        self.run(move |registry| registry.queue_cloud_storage(&id, enabled))
    }

    pub fn create(&self, name: &str, locator: Option<String>) -> RegistryResult<LibraryEntry> {
        let name = name.to_owned();
        self.run(move |registry| registry.create(&name, locator))
    }

    /// Persist the policy before the new library becomes visible to background work.
    pub fn create_with_asset_storage(&self, name: &str, locator: Option<String>, asset_storage_enabled: bool) -> RegistryResult<LibraryEntry> {
        let name = name.to_owned();
        self.run(move |registry| registry.create_with_asset_storage(&name, locator, asset_storage_enabled))
    }

    pub fn attach_remote(&self, id: &LibraryId, name: &str, library_root: &str) -> RegistryResult<LibraryEntry> {
        let id = *id;
        let name = name.to_owned();
        let library_root = library_root.to_owned();
        self.run(move |registry| registry.attach_remote(&id, &name, &library_root))
    }

    /// Platform fallback used when no destination preference has been saved.
    pub fn default_library_root(&self) -> &str {
        &self.0.default_library_root
    }

    pub fn is_detached(&self, library_id: &LibraryId) -> RegistryResult<bool> {
        let library_id = *library_id;
        self.run(move |registry| registry.is_detached(&library_id))
    }

    pub fn pending_server_deletions(&self) -> RegistryResult<Vec<LibraryId>> {
        self.run(move |registry| registry.pending_server_deletions())
    }

    pub fn complete_server_deletion(&self, library_id: &LibraryId) -> RegistryResult<()> {
        let library_id = *library_id;
        self.run(move |registry| registry.complete_server_deletion(&library_id))
    }

    /// Hide the library and persist both server and local deletion intents
    /// before physical cleanup can be interrupted.
    pub fn begin_local_removal(&self, library_id: &LibraryId) -> RegistryResult<()> {
        let library_id = *library_id;
        self.run(move |registry| registry.begin_local_removal(&library_id))
    }

    pub fn pending_local_purges(&self) -> RegistryResult<Vec<LibraryId>> {
        self.run(move |registry| registry.pending_local_purges())
    }

    /// Physical cleanup is idempotent. Keep the purge record if any step fails.
    pub fn complete_local_purge(&self, library_id: &LibraryId) -> RegistryResult<()> {
        let library_id = *library_id;
        self.run(move |registry| registry.complete_local_purge(&library_id))
    }

    /// Discovers libraries exposed by the current platform and registers any
    /// new entries. Platforms without directory discovery return an explicit
    /// capability error from the adapter; registration behavior stays shared.
    pub fn discover(&self, parent: &Path) -> RegistryResult<()> {
        let parent = parent.to_owned();
        self.run(move |registry| registry.discover(&parent))
    }

    pub fn remove(&self, library_id: &LibraryId) -> RegistryResult<()> {
        let library_id = *library_id;
        self.run(move |registry| registry.remove(&library_id))
    }

    pub fn read_value(&self, key: &str) -> RegistryResult<Option<Vec<u8>>> {
        let key = key.to_owned();
        self.run(move |registry| registry.read_value(&key))
    }

    /// Compare and replace device credentials in one owner command. `None`
    /// preserves a newer login or a connection removed during token refresh.
    pub fn update_value(&self, key: &str, update: impl FnOnce(Option<&[u8]>) -> RegistryResult<Option<Vec<u8>>> + Send + 'static) -> RegistryResult<bool> {
        let key = key.to_owned();
        self.run(move |registry| {
            let tx = registry.connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let current: Option<Vec<u8>> = tx.query_row("SELECT value FROM app_value WHERE key=?1", [&key], |row| row.get(0)).optional()?;
            let Some(value) = update(current.as_deref())? else {
                return Ok(false);
            };
            tx.execute(include_str!("sql/set_sync_disabled.sql"), params![key, value])?;
            tx.commit()?;
            Ok(true)
        })
    }

    pub fn write_value(&self, key: &str, value: &[u8]) -> RegistryResult<()> {
        let key = key.to_owned();
        let value = value.to_vec();
        self.run(move |registry| registry.write_value(&key, &value))
    }

    pub fn remove_value(&self, key: &str) -> RegistryResult<()> {
        let key = key.to_owned();
        self.run(move |registry| registry.remove_value(&key))
    }
}
