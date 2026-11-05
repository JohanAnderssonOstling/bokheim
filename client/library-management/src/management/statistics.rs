//! Storage statistics across registered libraries, including unopened indexes.
use super::LibraryService;
use crate::library::StorageCopyProgress;
use crate::{BackendError, LibraryId};
use futures_util::{stream, StreamExt, TryStreamExt};
use library_database::{Database, DatabaseError};
use library_model::CloudQuotaBook;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

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
                let mut libraries = Vec::new();
                for entry in service.list_on_storage()? {
                    let id = *entry.library_id();
                    let Some(library) = service.try_handle(&id) else { continue };
                    let journal: Option<StorageCopyJournal> = service.config.registry.read_value(&format!("storage-copy:v1:{id}:1"))?.map(|bytes| serde_json::from_slice(&bytes)).transpose().map_err(BackendError::operation)?;
                    libraries.push((entry, library, journal));
                }
                Ok(libraries)
            })
            .await?;
        let started = web_time::Instant::now();
        let entries: Vec<Option<LocalLibraryStorageUsage>> = stream::iter(libraries.into_iter().map(|(entry, library, journal)| async move {
            let books: Vec<CloudQuotaBook> = library.cloud_quota_books().await?;
            let cloud_bytes = books.iter().try_fold(0_u64, |bytes, book| bytes.checked_add(book.size_bytes));
            let cloud_copy_bytes = cloud_copy_remaining_bytes(&books, journal.as_ref());
            Ok::<_, BackendError>(library.local_storage_usage().await?.map(|used_bytes| LocalLibraryStorageUsage { library_id: *entry.library_id(), library_name: entry.library_name().to_owned(), used_bytes, cloud_bytes, cloud_copy_bytes }))
        }))
        .buffered(4)
        .try_collect()
        .await?;
        let mut usage: Vec<_> = entries.into_iter().flatten().collect();
        usage.sort_by(|left, right| left.library_name.cmp(&right.library_name));
        log::debug!("account local storage usage took {:?}", started.elapsed());
        Ok(usage)
    }
}

fn cloud_copy_remaining_bytes(books: &[CloudQuotaBook], journal: Option<&StorageCopyJournal>) -> Option<u64> {
    books.iter().filter(|source_book| !journal.is_some_and(|journal| journal.progress.books.iter().any(|book| book.source == source_book.book_id))).try_fold(0_u64, |bytes, book| bytes.checked_add(book.size_bytes))
}
