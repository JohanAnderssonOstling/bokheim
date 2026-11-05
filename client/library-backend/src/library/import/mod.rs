//! Catalog, reader-library, filesystem, and selected-file imports.

use super::{LibraryHandle, LibraryResources};
use crate::{BackendError, BookId, DirId};
use crate::{DirectoryImport, ImportFailure};
use client_platform_runtime::executor::{BackendOutput, BackendSend};
use client_protocol::Failure;
#[cfg(not(target_arch = "wasm32"))]
use library_import::reader_import::ReaderLibrary;
use library_model::{CardKey, LibraryBusyReason, LibraryUpdate};
use library_placement::LibraryState;
use std::{future::Future, sync::Arc};

#[cfg(not(target_arch = "wasm32"))]
mod browser;
pub(super) mod filesystem_scan;
#[cfg(all(feature = "watcher", not(target_arch = "wasm32")))]
pub(super) mod watcher;
#[cfg(target_arch = "wasm32")]
mod web;

impl LibraryHandle {
    /// Imports execute on the backend executor using a snapshot of the storage
    /// handles. Browsing and other actions do not wait for the complete import.
    pub(super) async fn run_import<T: BackendOutput, F: Future<Output = Result<T, BackendError>> + BackendSend + 'static>(
        &self, operation: impl FnOnce(Arc<LibraryResources>, LibraryState) -> F + BackendSend + 'static,
    ) -> Result<T, BackendError> {
        let handle = self.clone();
        self.owner
            .executor
            .spawn(async move {
                handle
                    .run_io(async {
                        let library = handle.owner.resources.clone();
                        let _activity = library.activity.start(Some(LibraryBusyReason::Importing));
                        let storage = handle.storage().await?;
                        operation(library, (*storage).clone()).await
                    })
                    .await
            })
            .await
            .map_err(BackendError::operation)?
    }

    /// Imports `directory` under `parent_id`, as a folder of its own when
    /// `create_root` is set and otherwise as loose books.
    pub async fn import_directory(&self, parent_id: DirId, directory: DirectoryImport, create_root: bool) -> Result<Vec<ImportFailure>, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.import_selected_directory(parent_id, directory, create_root).await
        }
        #[cfg(target_arch = "wasm32")]
        web::import_directory(self, parent_id, directory, create_root).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_reader(&self, source: crate::ReaderImportSource, options: crate::ReaderImportOptions) -> Result<crate::ReaderImportReport, BackendError> {
        self.run_import(move |library, storage| async move { Ok(ReaderLibrary::load_default(source).await?.import(&library.import(&storage), options).await?) }).await
    }

    pub async fn add_catalog_book(&self, parent_id: DirId, book_id: BookId) -> Result<crate::BookId, BackendError> {
        self.add_catalog_entry(parent_id, CardKey::from(book_id), false).await
    }

    pub async fn add_external_catalog_audiobook(&self, parent_id: DirId, entry: CardKey) -> Result<crate::BookId, BackendError> {
        self.add_catalog_entry(parent_id, entry, true).await
    }

    async fn add_catalog_entry(&self, parent_id: DirId, entry: CardKey, external: bool) -> Result<BookId, BackendError> {
        self.run_import(move |library, storage| async move {
            let book = storage
                .database()
                .catalog_entry(entry)
                .await?
                .filter(|book| book.hash.is_none() == external)
                .ok_or_else(|| BackendError::classified(Failure::NotFound, if external { "the audiobook is not in the catalog" } else { "the book is not in the catalog" }))?;
            let progress = |progress| library.events.updates.publish(LibraryUpdate::CatalogImport { entry, progress });
            let bytes = match book.hash {
                Some(hash) => library_catalog::download_book_with_progress(hash, &progress).await?,
                None => library_catalog::download_audiobook_with_progress(entry, &progress).await?,
            };
            library.import(&storage).import_catalog_download(parent_id, book, bytes).await
        })
        .await
    }

    pub async fn import_downloaded_book(&self, parent_id: DirId, file_name: String, bytes: Vec<u8>) -> Result<crate::BookId, BackendError> {
        self.run_import(move |library, storage| async move { Ok(library.import(&storage).import_downloaded_book(parent_id, &file_name, bytes).await?) }).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_paths(&self, parent_id: DirId, paths: Vec<std::path::PathBuf>, moving: bool) -> Result<Vec<crate::ImportFailure>, BackendError> {
        self.run_import(move |library, storage| async move { Ok(library.import(&storage).import_paths(parent_id, paths, moving).await?) }).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_stored_book(&self, parent_id: DirId, file_name: String, location: std::path::PathBuf) -> Result<crate::BookId, BackendError> {
        self.run_import(move |library, storage| async move { Ok(library.import(&storage).import_stored_book(parent_id, file_name, location).await?) }).await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn import_selected_files(
        &self, parent_id: DirId, create_root: bool, name: String, directories: Vec<Vec<String>>, paths: Vec<Vec<String>>, selected: client_platform_web::transport::import_io::SelectedFiles,
    ) -> Result<Vec<crate::ImportFailure>, BackendError> {
        self.run_import(move |library, storage| async move {
            Ok({
                use client_platform_web::transport::import_io::take_selected_file;
                let _selected = selected;
                let token = _selected.id().to_owned();
                let files = paths
                    .into_iter()
                    .enumerate()
                    .map(|(index, path)| {
                        let token = token.clone();
                        super::DirectoryImportFile { path, source: super::ImportSource::from_export(Some(Box::new(move || take_selected_file(&token, index)))) }
                    })
                    .collect();
                let result = library.import(&storage).import_selected_directory(parent_id, super::DirectoryImport { name, directories, files }, create_root).await;
                result?
            })
        })
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn release_selected_files(&self, token: String) {
        client_platform_runtime::executor::storage_cleanup(move || client_platform_web::transport::import_io::release_selected_files(&token));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn import_selected_directory(&self, parent_id: DirId, directory: DirectoryImport, create_root: bool) -> Result<Vec<ImportFailure>, BackendError> {
        self.run_import(move |library, storage| async move { library.import(&storage).import_selected_directory(parent_id, directory, create_root).await }).await
    }
}

impl LibraryResources {
    pub(crate) fn import<'a>(&self, storage: &'a LibraryState) -> library_import::LibraryImport<'a> {
        library_import::LibraryImport::new(storage, self.cpu.clone())
    }
}
