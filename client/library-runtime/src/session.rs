//! Coordination state shared by the work running inside one library session.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use library_replica::ScanProgress;
use uuid::Uuid;

use crate::events::{LibraryEvent, LibraryEventSender};
use crate::thumbnails::ThumbnailWork;

#[derive(Default)]
struct DirectoryImports {
    active: HashSet<Uuid>,
    progress: ScanProgress,
}

/// Mutable coordination state for one open library, independent of app policy
/// and of the storage implementation used by that library.
#[derive(Default)]
pub struct LibraryRuntime {
    directory_imports: Mutex<DirectoryImports>,
    local_work_running: AtomicBool,
    thumbnails: ThumbnailWork,
}

impl LibraryRuntime {
    pub fn has_directory_imports(&self) -> bool {
        !self.directory_imports.lock().unwrap_or_else(|error| error.into_inner()).active.is_empty()
    }

    /// Returns whether this was a newly registered import.
    pub fn begin_directory_import(&self, id: Uuid) -> bool {
        let mut imports = self.directory_imports.lock().unwrap_or_else(|error| error.into_inner());
        if imports.active.is_empty() {
            imports.progress = ScanProgress::default();
        }
        imports.active.insert(id)
    }

    /// Returns false for a late progress update from an import that has ended.
    pub fn advance_directory_import(&self, id: Uuid, total: u64, succeeded: u64, failed: u64) -> bool {
        let mut imports = self.directory_imports.lock().unwrap_or_else(|error| error.into_inner());
        if !imports.active.contains(&id) {
            return false;
        }
        imports.progress.total += total;
        imports.progress.succeeded += succeeded;
        imports.progress.failed += failed;
        true
    }

    pub fn directory_import_progress(&self) -> Option<ScanProgress> {
        let imports = self.directory_imports.lock().unwrap_or_else(|error| error.into_inner());
        (!imports.active.is_empty()).then_some(imports.progress)
    }

    /// Returns false for duplicate or late completion messages.
    pub fn finish_directory_import(&self, id: Uuid) -> bool {
        self.directory_imports.lock().unwrap_or_else(|error| error.into_inner()).active.remove(&id)
    }

    pub fn thumbnail_work_running(&self) -> bool {
        self.thumbnails.running()
    }

    pub fn thumbnail_batch_progress(&self) -> Option<ScanProgress> {
        self.thumbnails.progress()
    }

    pub fn thumbnail_batch_waiting(&self) -> bool {
        self.thumbnails.waiting()
    }

    pub fn thumbnail_generations_running(&self) -> usize {
        self.thumbnails.generations_running()
    }

    pub fn prepare_thumbnail_batch(&self, pending: u64) {
        self.thumbnails.prepare_batch(pending);
    }

    pub fn complete_thumbnail_batch_item(&self) {
        self.thumbnails.complete_item();
    }

    pub fn begin_thumbnail_generation(self: &Arc<Self>, events: LibraryEventSender) -> ThumbnailActivity {
        self.thumbnails.begin_generation();
        let _ = events.try_send(LibraryEvent::TransferStatusChanged);
        ThumbnailActivity(self.clone(), events)
    }

    pub fn begin_thumbnail_work(self: &Arc<Self>, events: LibraryEventSender) -> Option<LibraryActivity> {
        if !self.thumbnails.begin_batch() {
            return None;
        }
        let _ = events.try_send(LibraryEvent::TransferStatusChanged);
        Some(LibraryActivity { runtime: self.clone(), kind: LibraryActivityKind::ThumbnailWork, events })
    }

    pub fn begin_local_work(self: &Arc<Self>, events: LibraryEventSender) -> Option<LibraryActivity> {
        self.local_work_running.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok()?;
        let _ = events.try_send(LibraryEvent::TransferStatusChanged);
        Some(LibraryActivity { runtime: self.clone(), kind: LibraryActivityKind::LocalWork, events })
    }
}

#[derive(Clone, Copy)]
enum LibraryActivityKind {
    LocalWork,
    ThumbnailWork,
}

pub struct LibraryActivity {
    runtime: Arc<LibraryRuntime>,
    kind: LibraryActivityKind,
    events: LibraryEventSender,
}

impl Drop for LibraryActivity {
    fn drop(&mut self) {
        match self.kind {
            LibraryActivityKind::LocalWork => self.runtime.local_work_running.store(false, Ordering::Release),
            LibraryActivityKind::ThumbnailWork => self.runtime.thumbnails.finish_batch(),
        }
        let _ = self.events.try_send(LibraryEvent::TransferStatusChanged);
    }
}

pub struct ThumbnailActivity(Arc<LibraryRuntime>, LibraryEventSender);

impl Drop for ThumbnailActivity {
    fn drop(&mut self) {
        self.0.thumbnails.finish_generation();
        let _ = self.1.try_send(LibraryEvent::TransferStatusChanged);
    }
}
