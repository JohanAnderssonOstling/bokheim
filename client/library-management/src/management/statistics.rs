//! Storage statistics across registered libraries, including unopened indexes.
use super::LibraryService;
use crate::library::StorageCopyProgress;
use crate::{BackendError, LibraryId};
use futures_util::{stream, StreamExt};
use library_database::{Database, DatabaseError};
use library_model::CloudQuotaBook;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct LocalLibraryStorageUsage {
    pub library_id: LibraryId,
    pub library_name: String,
    pub used_bytes: Option<u64>,
    #[serde(default)]
    pub cloud_bytes: Option<u64>,
    #[serde(default)]
    pub cloud_copy_bytes: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StorageCopyJournal {
    pub destination: LibraryId,
    pub progress: StorageCopyProgress,
}

impl LibraryService {
    pub async fn text_search_index_bytes(self: &Arc<Self>) -> Result<u64, BackendError> {
        self.visit_text_indexes(false, Database::text_index_bytes_at).await
    }

    pub async fn cleanup_text_search_indexes(self: &Arc<Self>) -> Result<(), BackendError> {
        self.visit_text_indexes(true, |path| {
            Database::remove_text_index_at(path)?;
            Ok(0)
        })
        .await
        .map(|_| ())
    }

    async fn visit_text_indexes(self: &Arc<Self>, cleanup: bool, unopened: fn(&Path) -> Result<u64, DatabaseError>) -> Result<u64, BackendError> {
        let mut bytes = 0u64;
        let mut failure = None;
        for entry in self.run(|service| service.config.registry.entries().map_err(BackendError::operation)).await? {
            let id = *entry.library_id();
            let result: Result<u64, BackendError> = async {
                if let Some(library) = self.try_handle(&id) {
                    if cleanup {
                        library.cleanup_text_search_index().await
                    } else {
                        library.text_search_index_bytes().await
                    }
                } else {
                    self.run(move |service| {
                        let path = service.config.registry.library_database_locator(entry.library_id(), entry.storage_locator());
                        unopened(Path::new(&path)).map_err(BackendError::operation)
                    })
                    .await
                }
            }
            .await;
            match result {
                Ok(value) => bytes = bytes.saturating_add(value),
                Err(error) => {
                    log::warn!("could not process text search index for library {}: {error}", id);
                    failure.get_or_insert(error);
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(bytes),
        }
    }

    pub async fn local_storage_usage(self: &Arc<Self>) -> Result<Vec<LocalLibraryStorageUsage>, BackendError> {
        let libraries = self
            .run(|service| {
                service
                    .list_on_storage()?
                    .into_iter()
                    .map(|entry| {
                        let id = *entry.library_id();
                        let journal = service.config.registry.read_value(&format!("storage-copy:v1:{id}:1"))?.map(|bytes| serde_json::from_slice::<StorageCopyJournal>(&bytes)).transpose().map_err(BackendError::operation)?;
                        Ok((entry, service.try_handle(&id), journal))
                    })
                    .collect::<Result<Vec<_>, BackendError>>()
            })
            .await?;
        let mut usage = stream::iter(libraries.into_iter().map(|(entry, library, journal)| async move {
            let id = *entry.library_id();
            let observed: Result<_, BackendError> = async {
                if let Some(library) = library {
                    return Ok((library.local_storage_usage().await?, library.cloud_quota_books().await?));
                }
                #[cfg(not(target_arch = "wasm32"))]
                return self
                    .run(move |service| {
                        let entry = service.entry_on_storage(&id)?;
                        let path = service.config.registry.library_database_locator(&id, entry.storage_locator());
                        if !Path::new(&path).is_file() {
                            return Ok((None, Vec::new()));
                        }
                        let database = Database::open(path)?;
                        database.initialize_library_for_deferred_taxonomy(id)?;
                        Ok((database.local_book_storage_bytes()?, database.cloud_quota_books()?))
                    })
                    .await;
                #[cfg(target_arch = "wasm32")]
                Ok((None, Vec::new()))
            }
            .await;
            let (used_bytes, books) = observed.unwrap_or_else(|error| {
                log::warn!("could not read stored local usage for {id}: {error}");
                (None, Vec::new())
            });
            let cloud_bytes = books.iter().try_fold(0_u64, |bytes, book| bytes.checked_add(book.size_bytes));
            let cloud_copy_bytes = cloud_copy_remaining_bytes(&books, journal.as_ref());
            LocalLibraryStorageUsage { library_id: id, library_name: entry.library_name().to_owned(), used_bytes, cloud_bytes, cloud_copy_bytes }
        }))
        .buffered(4)
        .collect::<Vec<_>>()
        .await;
        usage.sort_by(|left, right| left.library_name.cmp(&right.library_name));
        Ok(usage)
    }
}

fn cloud_copy_remaining_bytes(books: &[CloudQuotaBook], journal: Option<&StorageCopyJournal>) -> Option<u64> {
    books.iter().filter(|source_book| !journal.is_some_and(|journal| journal.progress.books.iter().any(|book| book.source == source_book.book_id))).try_fold(0_u64, |bytes, book| bytes.checked_add(book.size_bytes))
}
