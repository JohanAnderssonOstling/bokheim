//! Browse-driven downloads and eviction over shared database queries.

use super::events::LibraryEvent;
use crate::library::LibrarySession;
use crate::{BackendError, BookId, DirId};
use book_model::BookFormat;
use client_platform_runtime::pending::OnDrop;
use library_model::{BookCardRow, BookFileActivity, BrowseLocation, BrowseRow, LibraryBrowseQuery};
use std::collections::HashSet;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LibraryBookStorage {
    pub local_bytes: u64,
    pub activity: BookFileActivity,
    pub download_book_ids: Vec<BookId>,
    pub download_bytes: u64,
    pub required_bytes: u64,
    pub removable_books: usize,
    pub removable_bytes: u64,
    pub available_bytes: Option<u64>,
    pub unknown_download_sizes: usize,
}

impl LibraryBookStorage {
    pub fn download_blocker(&self) -> Option<&'static str> {
        if self.unknown_download_sizes > 0 {
            Some("Download size unavailable.")
        } else if let Some(available) = self.available_bytes {
            (self.required_bytes > available).then_some("Not enough disk space.")
        } else {
            Some("Disk space unavailable.")
        }
    }
}

impl LibrarySession {
    /// The single admission policy used by commands and published UI snapshots.
    pub(super) fn book_file_activity(&self) -> Result<BookFileActivity, BackendError> {
        Ok(if self.runtime.bulk_book_operation_running.get() {
            BookFileActivity::BulkOperation
        } else if !self.db.book_download_requests(-1, None, 1)?.is_empty() {
            BookFileActivity::Downloading
        } else {
            BookFileActivity::Idle
        })
    }

    pub(super) fn check_book_file_changes(&self) -> Result<(), BackendError> {
        if self.book_file_activity()?.blocks_file_changes() {
            return Err(BackendError::message("wait for book-storage operations to finish before changing library files"));
        }
        Ok(())
    }

    pub(super) fn book_storage_activity(&self) -> Result<impl Drop + '_, BackendError> {
        self.check_book_file_changes()?;
        self.runtime.bulk_book_operation_running.set(true);
        self.events.emit(LibraryEvent::BookFileActivityChanged);
        Ok(OnDrop::new(|| {
            self.runtime.bulk_book_operation_running.set(false);
            self.events.emit(LibraryEvent::BookFileActivityChanged);
        }))
    }

    pub(crate) async fn library_book_storage(&self, for_download: bool) -> Result<LibraryBookStorage, BackendError> {
        let mut storage = LibraryBookStorage { activity: self.book_file_activity()?, ..Default::default() };
        let mut staging_bytes = 0;
        for book in self.db.cloud_quota_books()? {
            let paths = self.db.book_paths(&book.book_id)?;
            let bytes = self.assets.stored_book_bytes(book.book_id, paths.clone()).await.map_err(BackendError::operation)?;
            storage.local_bytes = storage.local_bytes.saturating_add(bytes);
            if !book.server_has_book {
                continue;
            }
            if !for_download {
                let removable = self.assets.evictable_book_bytes(book.book_id, paths).await.map_err(BackendError::operation)?;
                storage.removable_books += usize::from(removable > 0);
                storage.removable_bytes = storage.removable_bytes.saturating_add(removable);
            } else if !self.local_book_verified(book.book_id).await? {
                let size_bytes = if book.size_bytes == 0 { self.describe_remote_file(book.book_id).await.map_err(super::remote_access::range_backend_error)?.0.length } else { book.size_bytes };
                storage.download_book_ids.push(book.book_id);
                storage.unknown_download_sizes += usize::from(size_bytes == 0);
                storage.download_bytes = storage.download_bytes.saturating_add(size_bytes);
                let (retained, temporary) = self.assets.book_download_space(size_bytes, &paths, self.db.book_format(&book.book_id)? == Some(BookFormat::Mp3Folder));
                storage.required_bytes = storage.required_bytes.saturating_add(retained);
                staging_bytes = staging_bytes.max(temporary);
            }
        }
        storage.required_bytes = storage.required_bytes.saturating_add(staging_bytes);
        if for_download {
            storage.available_bytes = self.assets.available_write_capacity().await.map_err(BackendError::operation)?;
        }
        Ok(storage)
    }

    pub(crate) async fn download_all_books(&self, confirmed_books: Vec<BookId>, confirmed_bytes: u64) -> Result<(), BackendError> {
        let _activity = self.book_storage_activity()?;
        if !self.sync.asset_storage_enabled() || !self.sync.is_signed_in() {
            return Err(BackendError::message("sign in and enable cloud books before downloading"));
        }
        let storage = self.library_book_storage(true).await?;
        let confirmed = confirmed_books.into_iter().collect::<HashSet<_>>();
        if storage.required_bytes > confirmed_bytes || storage.download_book_ids.iter().any(|book| !confirmed.contains(book)) {
            return Err(BackendError::message("library changed; review the download size again"));
        }
        if let Some(reason) = storage.download_blocker() {
            return Err(BackendError::message(reason));
        }
        self.assets.check_write_capacity(storage.required_bytes).map_err(BackendError::operation)?;
        self.download_books_locally(&storage.download_book_ids).await
    }

    fn browse_hashes(&self, mut query: LibraryBrowseQuery, include_book: impl Fn(&BookCardRow) -> bool, descend: impl Fn(&BrowseRow) -> bool) -> Result<HashSet<BookId>, String> {
        let mut locations = vec![query.location];
        let mut visited = HashSet::new();
        let mut hashes = HashSet::new();
        while let Some(location) = locations.pop() {
            if !visited.insert(location) {
                continue;
            }
            query.location = location;
            let page = self.db.browse_contents(&query, false).map_err(|error| error.to_string())?;
            hashes.extend(page.books.into_iter().filter(&include_book).filter_map(|book| book.book_id()));
            locations.extend(page.children.into_iter().filter(&descend).map(|child| child.id));
        }
        Ok(hashes)
    }

    pub(crate) fn queue_browse_downloads(&self, query: LibraryBrowseQuery) -> Result<(), String> {
        let hashes = self.browse_hashes(query, |book| !book.downloaded && !book.download_requested, |child| child.downloaded_book_count < child.book_count)?;
        self.request_downloads(&hashes.into_iter().collect::<Vec<_>>()).map_err(|error| error.to_string())
    }

    /// Evicts the downloaded copies under one folder without deleting any
    /// entry. Only books the server also holds are evicted, so a local-only
    /// book keeps its bytes. Returns how many copies were evicted.
    pub(crate) async fn evict_folder_downloads(&self, directory_id: &DirId) -> Result<usize, BackendError> {
        let _activity = self.book_storage_activity()?;
        self.check_book_eviction()?;
        let query =
            LibraryBrowseQuery { location: BrowseLocation::Folder(*directory_id), search: String::new(), file_types: Vec::new(), chip_sort: Default::default(), book_sort: Default::default(), languages: Vec::new(), hide_finished: false };
        let hashes = self.browse_hashes(query, |book| book.downloaded, |child| child.downloaded_book_count > 0).map_err(BackendError::message)?;
        let remote = self.db.remote_asset_hashes(library_model::BlobKind::Book)?;
        let mut evicted = 0;
        for book_id in hashes {
            if !remote.contains(&book_id) {
                continue;
            }
            if !self.evict_local_book(book_id).await? {
                continue;
            }
            evicted += 1;
            tokio::task::yield_now().await;
        }
        Ok(evicted)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
