use super::core::*;
use crate::{LibraryFolderError, LibraryFolderStep};
use library_backend::library::handle::{LibraryActorCommand, LibraryControl};
use library_backend::library::LibraryRuntimeSpec;
use library_backend::{LibraryCommand, PendingCommand, ReadyCommand};
use library_model::CloudQuotaBook;
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::sync::Arc;
use sync_common::api::assets::BlobManifestEntry;

fn check_library_folder(path: &Path) -> Result<PathBuf, (LibraryFolderStep, BackendError)> {
    let access_error = |error: std::io::Error| {
        let error = if error.kind() == ErrorKind::NotFound { BackendError::classified(Failure::LibraryFolderUnavailable, error.to_string()) } else { BackendError::from(error) };
        (LibraryFolderStep::CheckFolder, error)
    };
    let metadata = fs::metadata(path).map_err(access_error)?;
    if !metadata.is_dir() {
        return Err((LibraryFolderStep::NotAFolder, BackendError::message("selected path is not a directory")));
    }
    // is_dir() hides access errors. Opening the listing keeps permission and
    // unavailable-drive failures distinct without modifying the user's files.
    let _ = fs::read_dir(path).map_err(access_error)?;
    path.canonicalize().map_err(access_error)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../tests/app/libraries/folder_add_tests.rs"]
mod folder_add_tests;

impl AppBackend {
    pub(crate) fn libraries(&self) -> Result<Vec<LibraryEntry>, BackendError> {
        self.state.library_service.list()
    }

    pub(super) fn open_registered_libraries(&self) -> Result<(), BackendError> {
        self.state.library_service.open_registered()
    }

    /// Registers folders selected by a native frontend.
    ///
    /// Paths remain inside the backend after this call. Frontends identify the
    /// resulting libraries only by `LibraryId`.
    pub(crate) fn create_libraries_from_paths_with_asset_storage(&self, paths: Vec<PathBuf>, asset_storage_enabled: bool) -> Result<(Vec<LibraryId>, Vec<LibraryFolderError>), BackendError> {
        let mut known_paths = self.libraries()?.into_iter().map(|entry| (PathBuf::from(entry.storage_locator()), *entry.library_id())).collect::<HashMap<_, _>>();
        let mut created_ids = Vec::new();
        let mut errors = Vec::new();
        for path in paths {
            let display_path = path.display().to_string();
            let failure = |step, error: BackendError| LibraryFolderError { folder_name: Some(library_name_for_folder(&path)), step, error: error.context(display_path.clone()) };
            let canonical_path = match check_library_folder(&path) {
                Ok(path) => path,
                Err((step, error)) => {
                    errors.push(failure(step, error));
                    continue;
                }
            };
            if let Some(id) = known_paths.get(&canonical_path) {
                // A prior attempt may have saved the registry entry but failed
                // to open its local data. Adding it again must retry that open.
                match self.open_registered_library(id) {
                    Ok(()) => {
                        if !created_ids.contains(id) {
                            created_ids.push(*id);
                        }
                        self.send_library_list_changed();
                    }
                    Err(error) => errors.push(failure(LibraryFolderStep::OpenLibrary, error)),
                }
                continue;
            }
            #[cfg(not(target_arch = "wasm32"))]
            if let Err(error) = library_registry::read_library_manifest(&canonical_path) {
                errors.push(failure(LibraryFolderStep::ReadLibraryInfo, BackendError::operation(error)));
                continue;
            }
            let entry = match self.state.library_service.register_with_asset_storage(&library_name_for_folder(&path), Some(canonical_path.to_string_lossy().into_owned()), asset_storage_enabled) {
                Ok(entry) => entry,
                Err(error) => {
                    errors.push(failure(LibraryFolderStep::SaveLibraryInfo, error));
                    continue;
                }
            };
            let opened = self.state.library_service.finish_created(entry);
            match opened {
                Ok(entry) => {
                    // Opening the library already queued its filesystem scan.
                    known_paths.insert(PathBuf::from(entry.storage_locator()), *entry.library_id());
                    created_ids.push(*entry.library_id());
                }
                Err(error) => errors.push(failure(LibraryFolderStep::OpenLibrary, error)),
            }
        }
        Ok((created_ids, errors))
    }

    pub(crate) async fn remove_library_from_device(&self, library_id: &LibraryId) -> Result<(), BackendError> {
        let id = *library_id;
        self.with_storage(move |this| {
            this.library_entry(&id)?;
            this.state.registry.begin_local_removal(&id).map_err(BackendError::operation)?;
            this.state.library_directory.retire(&id)?;
            Ok(())
        })?;
        self.send_library_list_changed();
        self.state.library_service.finish_local_purge(*library_id).await?;
        Ok(())
    }

    pub(super) fn start_pending_local_purges(&self) {
        let weak = Arc::downgrade(&self.state);
        self.state.executor.spawn_detached(async move {
            loop {
                let Some(state) = weak.upgrade() else { break };
                let pending = state.registry.pending_local_purges();
                drop(state);
                match pending {
                    Ok(pending) => {
                        for library_id in pending {
                            let Some(state) = weak.upgrade() else { return };
                            if let Err(error) = state.library_service.finish_local_purge(library_id).await {
                                log::warn!("local library purge for {library_id} will retry: {error}");
                            }
                        }
                    }
                    Err(error) => log::warn!("could not read pending local library purges: {error}"),
                }
                if let Err(error) = crate::executor::sleep(std::time::Duration::from_secs(30)).await {
                    log::warn!("local library purge retries stopped: {error}");
                    break;
                }
            }
        });
    }

    pub(crate) fn rename_library(&self, library_id: &LibraryId, name: &str) -> Result<(), BackendError> {
        let library_id = *library_id;
        let name = name.to_owned();
        self.with_storage(move |this| {
            // Require an existing library; renaming must never recreate a deleted one.
            this.library_entry(&library_id)?;
            // Updates both registry metadata and the native manifest, preserving storage.
            let entry = this.state.registry.attach_remote(&library_id, &name).map_err(BackendError::operation)?;
            let device_only = this.state.registry.device_only(&library_id).map_err(BackendError::operation)?;
            if !device_only {
                this.state.registry.write_value(&format!("pending-library-rename/{library_id}"), entry.library_name().as_bytes()).map_err(BackendError::operation)?;
            }
            if let Ok(handle) = this.state.library_directory.get(&library_id) {
                let _ = handle.commands.try_send(LibraryActorCommand::Control(LibraryControl::Rename { name: entry.library_name().to_owned(), refresh_remote: !device_only }));
            }
            this.send_library_list_changed();
            this.state.account_wake.send_replace(());
            Ok(())
        })
    }

    pub(crate) async fn delete_library(&self, library_id: &LibraryId) -> Result<(), BackendError> {
        let _location = self.state.library_location_lock.lock().await;
        if self.library_entry(library_id)?.is_opds() {
            self.remove_library_from_device(library_id).await?;
            #[cfg(not(target_arch = "wasm32"))]
            self.state.catalog.forget_credentials(*library_id)?;
            self.state.account_wake.send_replace(());
            return Ok(());
        }
        self.library_entry(library_id)?;
        let device_only = self.state.registry.device_only(library_id).map_err(BackendError::operation)?;
        if self.state.library_service.current_account()?.is_none() && self.state.registry.has_account(library_id).map_err(BackendError::operation)? {
            return Err(BackendError::classified(Failure::SignInRequired, "sign in to save the full library locally before removing it from cloud"));
        }
        if !device_only && self.state.library_service.current_account()?.is_some() {
            self.make_library_local_only(*library_id).await?;
            return Ok(());
        }
        // Local actions never depend on network/account availability. Removal
        // records a durable server-deletion intent before discarding local state.
        self.remove_library_from_device(library_id).await?;
        if device_only {
            self.state.registry.complete_server_deletion(library_id).map_err(BackendError::operation)?;
        }
        Ok(())
    }

    pub(super) fn open_registered_library(&self, library_id: &LibraryId) -> Result<(), BackendError> {
        self.state.library_service.open(library_id)
    }

    pub(crate) async fn set_library_asset_storage(&self, id: LibraryId, enabled: bool) -> Result<(), BackendError> {
        if self.library_entry(&id)?.is_opds() {
            return Err("OPDS libraries use their own server".into());
        }
        let _storage_change = self.state.storage_copy_lock.lock().await;
        let configuration = self.state.registry.configuration(&id).map_err(BackendError::operation)?;
        let was_enabled = configuration.storage().uses_bokheim_assets();
        if enabled == was_enabled && self.state.registry.read_value(&format!("cloud-storage-reset:{id}")).map_err(BackendError::operation)?.is_none() {
            return Ok(());
        }
        let account = self.state.library_service.account_identity()?;
        let library = self.state.library_directory.get(&id)?;
        let _quota = if enabled && !was_enabled {
            let books: Vec<CloudQuotaBook> = library.dispatch(LibraryCommand::Ready(ReadyCommand::CloudQuotaBooks)).await?.take()?;
            Some(self.state.library_service.admit_cloud_books(books.into_iter().map(|book| BlobManifestEntry { byte_checksum: book.byte_checksum, book_id: book.book_id, size_bytes: book.size_bytes }).collect()).await?)
        } else {
            None
        };
        let _retained_books = if !enabled && was_enabled && !configuration.device_only() && (account.is_some() || configuration.has_account()) {
            let retention = library.retain_local_books().await?;
            library.dispatch(LibraryCommand::Pending(PendingCommand::SaveBooksLocally)).await?.take::<()>()?;
            Some(retention)
        } else {
            None
        };
        let reset = self.with_storage(move |this| {
            this.state.library_service.check_account_identity(&account)?;
            this.state.registry.configure_cloud_storage(&id, enabled, account.as_deref()).map_err(BackendError::operation)?;
            this.state.account_wake.send_replace(());
            this.state.registry.read_value(&format!("cloud-storage-reset:{id}")).map_err(BackendError::operation)
        })?;
        // Acknowledge the derived runtime policy; keep the durable reset intent
        // if applying it fails so reopening can recover from the configuration.
        library.dispatch(LibraryCommand::Ready(ReadyCommand::ApplyStoragePolicy { enabled, reset_cloud_presence: true })).await?.take::<()>()?;
        if let Some(reset) = reset {
            self.state.registry.clear_value_if_current(&format!("cloud-storage-reset:{id}"), &reset).map_err(BackendError::operation)?;
        }
        Ok(())
    }
}

impl AppBackend {
    pub(super) fn library_runtime_spec(&self, library_id: LibraryId) -> Result<(LibraryRuntimeSpec, Option<Vec<u8>>), BackendError> {
        self.state.library_service.runtime_spec(library_id)
    }
}
