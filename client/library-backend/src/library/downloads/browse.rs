//! Browse-driven downloads and eviction over shared database queries.

use crate::library::LibraryResources;
use crate::{BackendError, BookId, DirId};
use library_database::assets::BookStorageMetadata;
use library_database::{DatabaseClient, DatabaseError};
use library_model::{BookCardRow, BookFileActivity, BrowseLocation, BrowseRow, LibraryBrowseQuery};
use library_placement::LibraryState;
use std::collections::HashSet;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LibraryBookStorage {
    pub local_bytes: u64,
    pub activity: BookFileActivity,
    pub download_book_ids: Vec<BookId>,
    pub download_bytes: Option<u64>,
    pub removable_books: usize,
    pub removable_bytes: u64,
}

impl LibraryResources {
    /// Combine foreground restrictions with durable download requests.
    pub(in crate::library) async fn book_file_activity(&self) -> Result<BookFileActivity, BackendError> {
        let activity = self.activity.snapshot(false);
        if activity.reason().is_some() {
            return Ok(activity);
        }
        let downloading = !self.database.book_download_requests(-1, None, 1).await?.is_empty();
        // Foreground work can start while the database request is pending.
        Ok(self.activity.snapshot(downloading))
    }

    pub(crate) async fn library_book_storage(&self, for_download: bool) -> Result<LibraryBookStorage, BackendError> {
        let assets = self.state.read().await.assets().clone();
        let activity = self.book_file_activity().await?;
        // Inspect files after the database method has released its snapshot.
        let books = self.database.book_storage_metadata(for_download).await?;
        let mut storage = LibraryBookStorage { activity, download_bytes: Some(0), ..Default::default() };
        for BookStorageMetadata { book, paths, checksum } in books {
            let bytes = assets.stored_book_bytes(book.book_id, paths.clone()).await.map_err(BackendError::operation)?;
            storage.local_bytes = storage.local_bytes.saturating_add(bytes);
            if !book.server_has_book {
                continue;
            }
            if !for_download {
                let removable = assets.evictable_book_bytes(book.book_id, paths).await.map_err(BackendError::operation)?;
                storage.removable_books += usize::from(removable > 0);
                storage.removable_bytes = storage.removable_bytes.saturating_add(removable);
            } else if assets.verified_book_reader_with_checksum(book.book_id, paths, checksum).await.map_err(BackendError::operation)?.is_none() {
                storage.download_book_ids.push(book.book_id);
                storage.download_bytes = storage.download_bytes.and_then(|bytes| (book.size_bytes > 0).then_some(book.size_bytes).and_then(|size| bytes.checked_add(size)));
            }
        }
        Ok(storage)
    }

    pub(crate) async fn queue_browse_downloads(&self, storage: &LibraryState, query: LibraryBrowseQuery) -> Result<(), BackendError> {
        let hashes = browse_hashes(storage.database(), query, |book| !book.downloaded && !book.download_requested, |child| child.downloaded_book_count < child.book_count).await?;
        self.request_downloads(&hashes.into_iter().collect::<Vec<_>>()).await
    }

    /// Evicts the downloaded copies under one folder without deleting any
    /// entry. Only books the server also holds are evicted, so a local-only
    /// book keeps its bytes. Returns how many copies were evicted.
    pub(crate) async fn evict_folder_downloads(&self, storage: &LibraryState, directory_id: &DirId) -> Result<usize, BackendError> {
        self.check_book_eviction()?;
        let query =
            LibraryBrowseQuery { location: BrowseLocation::Folder(*directory_id), search: String::new(), file_types: Vec::new(), chip_sort: Default::default(), book_sort: Default::default(), languages: Vec::new(), hide_finished: false };
        let hashes = browse_hashes(storage.database(), query, |book| book.downloaded, |child| child.downloaded_book_count > 0).await?;
        let remote = storage.database().remote_asset_hashes(library_model::BlobKind::Book).await?;
        let mut evicted = 0;
        for book_id in hashes {
            if !remote.contains(&book_id) {
                continue;
            }
            if !self.evict_local_book(storage, book_id).await? {
                continue;
            }
            evicted += 1;
            tokio::task::yield_now().await;
        }
        Ok(evicted)
    }
}

async fn browse_hashes(db: &DatabaseClient, mut query: LibraryBrowseQuery, include_book: impl Fn(&BookCardRow) -> bool, descend: impl Fn(&BrowseRow) -> bool) -> Result<HashSet<BookId>, DatabaseError> {
    let mut locations = vec![query.location];
    let mut visited = HashSet::new();
    let mut hashes = HashSet::new();
    while let Some(location) = locations.pop() {
        if !visited.insert(location) {
            continue;
        }
        query.location = location;
        let page = db.browse_contents(&query, false).await?;
        hashes.extend(page.books.into_iter().filter(&include_book).filter_map(|book| book.book_id()));
        locations.extend(page.children.into_iter().filter(&descend).map(|child| child.id));
    }
    Ok(hashes)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
