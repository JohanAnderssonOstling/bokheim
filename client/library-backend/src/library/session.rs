//! Coordination state owned by one library session.

#[cfg(not(target_arch = "wasm32"))]
use super::LibrarySession;
use crate::{BackendError, BookId};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use uuid::Uuid;

/// Mutable coordination state for one open library, independent of app policy
/// and of the storage implementation used by that library.
#[derive(Default)]
pub struct LibraryRuntime {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) database_cache_users: RefCell<HashSet<Uuid>>,
    #[cfg(not(target_arch = "wasm32"))]
    database_cache_work: Cell<usize>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) automatic_export: super::automatic_annotation_export::AutomaticExport,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) annotation_export_lock: tokio::sync::Mutex<()>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) annotation_exports: super::job_registry::JobRegistry<library_model::AnnotationExportProgress, library_annotation_export::ExportControl>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) reader_imports: super::job_registry::JobRegistry<library_import::reader_import::ReaderImportProgress, Rc<Cell<bool>>>,
    pub(crate) upload_quota_blocked: Cell<bool>,
    pub(crate) bulk_book_operation_running: Cell<bool>,
    pub(crate) local_book_retentions: RefCell<HashSet<Uuid>>,
    local_work_running: Cell<bool>,
    thumbnail_work_running: Cell<bool>,
    catalog_cover_refreshes: RefCell<HashSet<BookId>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl LibrarySession {
    pub(crate) fn release_inactive_database_cache(&self) {
        if !self.runtime.database_cache_users.borrow().is_empty() || self.runtime.database_cache_work.get() != 0 || self.directory_imports_active() {
            return;
        }
        if let Err(error) = self.db.release_cached_memory() {
            log::warn!("could not release cached database memory for library {}: {error}", self.id);
        }
    }

    pub(crate) fn database_cache_work(&self) -> DatabaseCacheWork<'_> {
        self.runtime.database_cache_work.set(self.runtime.database_cache_work.get() + 1);
        DatabaseCacheWork { session: self }
    }
}

/// Keep worker caches until overlapping passes have all completed.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct DatabaseCacheWork<'a> {
    session: &'a LibrarySession,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for DatabaseCacheWork<'_> {
    fn drop(&mut self) {
        let work = &self.session.runtime.database_cache_work;
        work.set(work.get() - 1);
        self.session.release_inactive_database_cache();
    }
}

impl LibraryRuntime {
    pub(crate) fn check_book_eviction(&self) -> Result<(), BackendError> {
        if !self.local_book_retentions.borrow().is_empty() {
            return Err(BackendError::message("wait for the cloud storage change to finish before removing downloads"));
        }
        Ok(())
    }

    pub(crate) fn claim_catalog_cover_refresh(&self, hash: BookId) -> bool {
        self.catalog_cover_refreshes.borrow_mut().insert(hash)
    }

    pub(crate) fn retry_catalog_cover_refresh(&self, hash: &BookId) {
        self.catalog_cover_refreshes.borrow_mut().remove(hash);
    }

    /// Starts the one thumbnail batch; None while another batch runs.
    pub fn begin_thumbnail_work(self: &Rc<Self>) -> Option<LibraryActivity> {
        if self.thumbnail_work_running.replace(true) {
            return None;
        }
        Some(LibraryActivity { runtime: self.clone(), kind: LibraryActivityKind::ThumbnailWork })
    }

    pub(crate) fn local_work_running(&self) -> bool {
        self.local_work_running.get()
    }

    /// Starts local file replay; None while another replay runs.
    pub fn begin_local_work(self: &Rc<Self>) -> Option<LibraryActivity> {
        if self.local_work_running.replace(true) {
            return None;
        }
        Some(LibraryActivity { runtime: self.clone(), kind: LibraryActivityKind::LocalWork })
    }
}

#[derive(Clone, Copy)]
pub(crate) enum LibraryActivityKind {
    LocalWork,
    ThumbnailWork,
}

pub struct LibraryActivity {
    pub(crate) runtime: Rc<LibraryRuntime>,
    pub(crate) kind: LibraryActivityKind,
}

impl Drop for LibraryActivity {
    fn drop(&mut self) {
        match self.kind {
            LibraryActivityKind::LocalWork => self.runtime.local_work_running.set(false),
            LibraryActivityKind::ThumbnailWork => self.runtime.thumbnail_work_running.set(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_cover_refresh_is_per_library_and_failed_attempts_can_retry() {
        let runtime = LibraryRuntime::default();
        let hash = BookId::new(&"a".repeat(64));
        assert!(runtime.claim_catalog_cover_refresh(hash));
        assert!(!runtime.claim_catalog_cover_refresh(hash));
        runtime.retry_catalog_cover_refresh(&hash);
        assert!(runtime.claim_catalog_cover_refresh(hash));
        assert!(LibraryRuntime::default().claim_catalog_cover_refresh(hash));
    }

    #[test]
    fn overlapping_work_is_rejected_and_cancellation_releases_its_lane() {
        let runtime = Rc::new(LibraryRuntime::default());
        let local = runtime.begin_local_work().unwrap();
        let thumbnails = runtime.begin_thumbnail_work().unwrap();
        assert!(runtime.begin_local_work().is_none());
        assert!(runtime.begin_thumbnail_work().is_none());
        drop(local);
        assert!(runtime.begin_local_work().is_some());
        assert!(runtime.begin_thumbnail_work().is_none());
        drop(thumbnails);
        assert!(runtime.begin_thumbnail_work().is_some());
    }
}
