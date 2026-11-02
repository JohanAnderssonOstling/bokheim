//! Storage statistics across registered libraries, including unopened indexes.
use super::LibraryService;
use crate::library::StorageCopyProgress;
use crate::{BackendError, LibraryCommand, LibraryId, PendingCommand, ReadyCommand};
use futures_util::{stream, StreamExt, TryStreamExt};
use library_database::{
    rusqlite::{Connection as SqlConnection, Error as SqlError, ErrorCode, OpenFlags},
    Database,
};
use library_model::CloudQuotaBook;
use serde::{Deserialize, Serialize};

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
    pub async fn text_search_index_bytes(&self) -> Result<u64, BackendError> {
        let mut bytes = 0u64;
        let mut failure = None;
        for entry in self.run(|service| service.state.registry.entries().map_err(BackendError::operation)).await? {
            let id = *entry.library_id();
            let result: Result<u64, BackendError> = async {
                if let Some(library) = self.run(move |service| service.state.library_directory.try_get(&id)).await? {
                    library.dispatch(LibraryCommand::Ready(ReadyCommand::TextSearchIndexBytes)).await?.take::<u64>()
                } else {
                    self.run(move |service| {
                        let path = service.state.registry.library_database_locator(entry.library_id(), entry.storage_locator());
                        match SqlConnection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE) {
                            Ok(probe) => {
                                drop(probe);
                                let db = Database::open(&path)?;
                                if !client_platform_runtime::text_search::enabled() {
                                    db.configure_text_index(false)?;
                                }
                                db.text_index_bytes().map_err(BackendError::operation)
                            }
                            Err(SqlError::SqliteFailure(error, _)) if error.code == ErrorCode::CannotOpen => Ok(0),
                            Err(error) => Err(BackendError::operation(error)),
                        }
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

    pub async fn local_storage_usage(&self) -> Result<Vec<LocalLibraryStorageUsage>, BackendError> {
        let libraries = self.run(|service| service.list()).await?;
        let started = web_time::Instant::now();
        let entries: Vec<Option<LocalLibraryStorageUsage>> = stream::iter(libraries.iter().map(|entry| async move {
            let id = *entry.library_id();
            let Ok(library) = self.run(move |service| service.state.library_directory.get(&id)).await else {
                return Ok::<_, BackendError>(None);
            };
            let books: Vec<CloudQuotaBook> = library.dispatch(LibraryCommand::Ready(ReadyCommand::CloudQuotaBooks)).await?.take()?;
            let cloud_bytes = books.iter().try_fold(0_u64, |bytes, book| bytes.checked_add(book.size_bytes));
            let cloud_copy_bytes = self.run(move |service| service.cloud_copy_remaining_bytes(id, &books)).await?;
            Ok(library.dispatch(LibraryCommand::Pending(PendingCommand::LocalStorageUsage)).await?.take::<Option<u64>>()?.map(|used_bytes| LocalLibraryStorageUsage {
                library_id: *entry.library_id(),
                library_name: entry.library_name().to_owned(),
                used_bytes,
                cloud_bytes,
                cloud_copy_bytes,
            }))
        }))
        .buffered(4)
        .try_collect()
        .await?;
        let mut usage: Vec<_> = entries.into_iter().flatten().collect();
        usage.sort_by(|left, right| left.library_name.cmp(&right.library_name));
        log::debug!("account local storage usage took {:?}", started.elapsed());
        Ok(usage)
    }

    fn cloud_copy_remaining_bytes(&self, source: LibraryId, books: &[CloudQuotaBook]) -> Result<Option<u64>, BackendError> {
        let journal: Option<StorageCopyJournal> =
            self.state.registry.read_value(&format!("storage-copy:v1:{source}:1")).map_err(BackendError::operation)?.map(|bytes| serde_json::from_slice(&bytes).map_err(BackendError::operation)).transpose()?;
        Ok(books.iter().filter(|source_book| !journal.as_ref().is_some_and(|journal| journal.progress.books.iter().any(|book| book.source == source_book.book_id))).try_fold(0_u64, |bytes, book| bytes.checked_add(book.size_bytes)))
    }
}

impl app_preferences::TextSearchIndexCleanup for LibraryService {
    fn schedule_cleanup(&self) {
        let libraries = self.clone();
        self.state.executor.spawn_detached(async move {
            if let Err(error) = libraries.text_search_index_bytes().await {
                log::warn!("could not finish removing text search indexes: {error}");
            }
        });
    }
}
