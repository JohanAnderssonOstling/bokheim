//! Native library-handle wrappers for shared browse queries.

use std::collections::HashSet;
use super::events::LibraryEvent;
use crate::library::LibrarySession;
use crate::BackendError;
use crate::DirId;
use library_model::BrowseContents;

impl LibrarySession {
    pub fn book_count(&self) -> Result<usize, BackendError> {
        ({
            use library_database::browse::{CountsSql, HomeSql};
            use library_database::DatabaseError;
            use library_model::LibraryHomeView;
            let database = &self.db;
            (|| -> Result<usize, DatabaseError> {
                let mut count = 0_i64;
                database.connection.count_all_books(|row| {
                    count = row.get(0)?;
                    Ok(())
                })?;
                Ok(usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative"))?)
            })()
        })
        .map_err(BackendError::operation)
    }

    pub fn browse_contents(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<BrowseContents, BackendError> {
        self.db.browse_contents(query, downloaded_only).map_err(BackendError::operation)
    }

    pub fn directory_path(&self, directory_id: &DirId) -> Result<Vec<library_model::BrowsePathSegment>, BackendError> {
        self.db.browse_directory_path(directory_id).map_err(BackendError::operation)
    }
}

impl LibrarySession {
    pub(crate) fn queue_browse_downloads(&self, query: library_model::LibraryBrowseQuery) -> Result<(), String> {
        let mut query = query;
        let mut locations = vec![query.location];
        let mut visited = HashSet::new();
        let mut hashes = HashSet::new();
        while let Some(location) = locations.pop() {
            if !visited.insert(location) {
                continue;
            }
            query.location = location;
            let page = self.db.browse_contents(&query, false).map_err(|error| error.to_string())?;
            hashes.extend(page.books.into_iter().filter(|book| !book.downloaded && !book.download_requested).map(|book| book.content_hash));
            locations.extend(page.children.into_iter().filter(|child| child.downloaded_book_count < child.book_count).map(|child| child.id));
        }
        let hashes = hashes.into_iter().collect::<Vec<_>>();
        self.request_downloads(&hashes).map_err(|error| error.to_string())
    }

    /// Evicts the downloaded copies under one folder without deleting any
    /// entry. Only books the server also holds are evicted, so a local-only
    /// book keeps its bytes. Returns how many copies were evicted.
    pub(crate) fn evict_folder_downloads(&self, directory_id: &DirId) -> Result<usize, String> {
        let mut query = library_model::LibraryBrowseQuery {
            location: library_model::BrowseLocation::Folder(*directory_id),
            search: String::new(),
            file_types: Vec::new(),
            chip_sort: Default::default(),
            book_sort: Default::default(),
            languages: Vec::new(),
            hide_finished: false,
            include_direct_child_books: false,
        };
        let mut locations = vec![query.location];
        let mut visited = HashSet::new();
        let mut hashes = HashSet::new();
        while let Some(location) = locations.pop() {
            if !visited.insert(location) {
                continue;
            }
            query.location = location;
            let page = self.db.browse_contents(&query, false).map_err(|error| error.to_string())?;
            hashes.extend(page.books.into_iter().filter(|book| book.downloaded).map(|book| book.content_hash));
            locations.extend(page.children.into_iter().filter(|child| child.downloaded_book_count > 0).map(|child| child.id));
        }
        let remote = self.db.remote_asset_hashes(library_model::BlobKind::Book).map_err(|error| error.to_string())?;
        let mut evicted = 0;
        for content_hash in hashes {
            if !remote.contains(&content_hash) {
                continue;
            }
            crate::asset_workflow::evict_local_book(&self.assets, &self.db, &content_hash).map_err(|error| error.to_string())?;
            let state = self.persistent_download_state(content_hash).map_err(|error| error.to_string())?;
            self.emit(LibraryEvent::DownloadStatusChanged { content_hash, state });
            evicted += 1;
        }
        if evicted > 0 {
            self.notify_contents_changed();
        }
        Ok(evicted)
    }
}
