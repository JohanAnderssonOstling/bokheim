use super::{LibraryFolderError, LibraryFolderStep, LibraryService};
use crate::{BackendError, LibraryId};
use client_protocol::Failure;
use library_model::CloudQuotaBook;
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
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

impl LibraryService {
    /// Registers folders selected by a native frontend.
    ///
    /// Paths remain inside the backend after this call. Frontends identify the
    /// resulting libraries only by `LibraryId`.
    pub async fn create_libraries_from_paths_with_asset_storage(self: &Arc<Self>, paths: Vec<PathBuf>, asset_storage_enabled: bool) -> Result<(Vec<LibraryId>, Vec<LibraryFolderError>), BackendError> {
        let mut known_paths = self.list().await?.into_iter().map(|entry| (PathBuf::from(entry.storage_locator()), *entry.library_id())).collect::<HashMap<_, _>>();
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
                match self.open(id).await {
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
            let entry = match self.register_with_asset_storage(&library_name_for_folder(&path), Some(canonical_path.to_string_lossy().into_owned()), asset_storage_enabled).await {
                Ok(entry) => entry,
                Err(error) => {
                    errors.push(failure(LibraryFolderStep::SaveLibraryInfo, error));
                    continue;
                }
            };
            let opened = self.finish_created(entry).await;
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

    pub fn start_pending_local_purges(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        self.config.executor.spawn_detached(async move {
            loop {
                let Some(state) = weak.upgrade() else { break };
                let pending = state.config.registry.access(|registry| registry.pending_local_purges()).await;
                drop(state);
                match pending {
                    Ok(pending) => {
                        for library_id in pending {
                            let Some(state) = weak.upgrade() else { return };
                            if let Err(error) = state.finish_local_purge(library_id).await {
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

    pub async fn delete_library(self: &Arc<Self>, library_id: &LibraryId) -> Result<(), BackendError> {
        let _location = self.config.library_location_lock.lock().await;
        let id = *library_id;
        // Removal needs registry metadata only, including for libraries whose
        // databases cannot open. Persist cloud deletion before local cleanup.
        self.run(move |this| {
            this.entry_on_storage(&id)?;
            this.bind_to_current_account_on_storage(&id)?;
            let device_only = this.config.registry.device_only(&id).map_err(BackendError::operation)?;
            this.config.registry.begin_local_removal(&id).map_err(BackendError::operation)?;
            this.retire(&id);
            if device_only {
                this.config.registry.complete_server_deletion(&id).map_err(BackendError::operation)?;
            }
            Ok(())
        })
        .await?;
        self.send_library_list_changed();
        self.config.account_wake.send_replace(());
        self.finish_local_purge(id).await?;
        Ok(())
    }

    pub async fn set_library_asset_storage(self: &Arc<Self>, id: LibraryId, enabled: bool) -> Result<(), BackendError> {
        let _storage_change = self.config.storage_copy_lock.lock().await;
        let configuration = self.config.registry.access(move |registry| registry.configuration(&id)).await.map_err(BackendError::operation)?;
        let was_enabled = configuration.storage().uses_bokheim_assets();
        if enabled == was_enabled && self.config.registry.access(move |registry| registry.cloud_storage_reset(&id)).await.map_err(BackendError::operation)?.is_none() {
            return Ok(());
        }
        let account = self.account_identity().await?;
        let library = self.handle(&id)?;
        let _quota = if enabled && !was_enabled {
            let books: Vec<CloudQuotaBook> = library.cloud_quota_books().await?;
            Some(self.admit_cloud_capacity(books.into_iter().map(|book| BlobManifestEntry { byte_checksum: book.byte_checksum, book_id: book.book_id, size_bytes: book.size_bytes }).collect(), 0).await?)
        } else {
            None
        };
        let save_books = !enabled && was_enabled && !configuration.device_only() && (account.is_some() || configuration.has_account());
        if save_books {
            self.ensure_remote_library(id).await?;
        }
        let management = self.clone();
        library
            .change_asset_storage(enabled, save_books, async move {
                management
                    .run(move |this| {
                        this.check_account_identity_on_storage(&account)?;
                        this.config.registry.configure_cloud_storage(&id, enabled, account.as_deref()).map_err(BackendError::operation)?;
                        this.config.account_wake.send_replace(());
                        Ok(())
                    })
                    .await
            })
            .await?;
        // Keep the durable reset intent if applying the runtime policy fails,
        // so reopening can recover from the committed configuration.
        if let Some(reset) = self.config.registry.access(move |registry| registry.cloud_storage_reset(&id)).await.map_err(BackendError::operation)? {
            self.config.registry.access(move |registry| registry.acknowledge_cloud_storage_reset(&id, reset)).await.map_err(BackendError::operation)?;
        }
        Ok(())
    }
}

fn library_name_for_folder(path: &Path) -> String {
    path.file_name().and_then(|name| name.to_str()).filter(|name| !name.trim().is_empty()).map(str::to_owned).unwrap_or_else(|| path.display().to_string())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "folder_tests.rs"]
mod folder_tests;
