//! Coordination state owned by one library session.

pub(crate) use std::collections::HashMap;
pub(crate) use std::sync::atomic::{AtomicBool, Ordering};
pub(crate) use std::sync::{Arc, Mutex};

pub(crate) use library_model::ScanProgress;
pub(crate) use uuid::Uuid;

pub(crate) use super::thumbnails::ThumbnailWork;
pub(crate) use library_runtime::events::{LibraryEvent, LibraryEventSender};

#[derive(Default)]
pub(crate) struct DirectoryImports {
    pub(crate) active: HashMap<Uuid, ImportState>,
    // Retain completed contributions until the overlapping group finishes.
    pub(crate) progress: ScanProgress,
}

#[derive(Default)]
pub(crate) struct ImportState {
    pub(crate) progress: ScanProgress,
    pub(crate) file: Option<Arc<Mutex<library_model::ImportFileProgress>>>,
}

/// Mutable coordination state for one open library, independent of app policy
/// and of the storage implementation used by that library.
#[derive(Default)]
pub struct LibraryRuntime {
    pub(crate) directory_imports: Mutex<DirectoryImports>,
    pub(crate) local_work_running: AtomicBool,
    pub(crate) thumbnails: ThumbnailWork,
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
        if let std::collections::hash_map::Entry::Vacant(entry) = imports.active.entry(id) {
            entry.insert(ImportState::default());
            true
        } else {
            false
        }
    }

    /// Returns false for a late progress update from an import that has ended.
    pub fn advance_directory_import(&self, id: Uuid, total: u64, succeeded: u64, failed: u64) -> bool {
        let mut imports = self.directory_imports.lock().unwrap_or_else(|error| error.into_inner());
        let Some(import) = imports.active.get_mut(&id) else { return false };
        import.progress.total += total;
        import.progress.succeeded += succeeded;
        import.progress.failed += failed;
        imports.progress.total += total;
        imports.progress.succeeded += succeeded;
        imports.progress.failed += failed;
        true
    }

    pub fn directory_import_progress(&self) -> Option<ScanProgress> {
        let imports = self.directory_imports.lock().unwrap_or_else(|error| error.into_inner());
        (!imports.active.is_empty()).then_some(imports.progress)
    }

    pub fn start_import_file(&self, id: Uuid, name: String, total_bytes: Option<u64>) -> Arc<Mutex<library_model::ImportFileProgress>> {
        let file = Arc::new(Mutex::new(library_model::ImportFileProgress { name, total_bytes, stage: library_model::ImportFileStage::Preparing }));
        let mut imports = self.directory_imports.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(import) = imports.active.get_mut(&id) {
            import.file = Some(file.clone());
        }
        file
    }

    pub fn finish_import_file(&self, id: Uuid) {
        if let Some(import) = self.directory_imports.lock().unwrap_or_else(|e| e.into_inner()).active.get_mut(&id) {
            import.file = None;
        }
    }

    pub fn directory_import_details(&self) -> Option<library_model::DirectoryImportProgress> {
        self.directory_import_details_for(None)
    }

    pub fn directory_import_details_for(&self, id: Option<Uuid>) -> Option<library_model::DirectoryImportProgress> {
        let imports = self.directory_imports.lock().unwrap_or_else(|e| e.into_inner());
        let (progress, file) = match id {
            Some(id) => {
                let import = imports.active.get(&id)?;
                (import.progress, import.file.as_ref())
            }
            None if !imports.active.is_empty() => (imports.progress, imports.active.values().find_map(|import| import.file.as_ref())),
            None => return None,
        };
        Some(library_model::DirectoryImportProgress { total: progress.total, succeeded: progress.succeeded, failed: progress.failed, current: file.map(|file| file.lock().unwrap_or_else(|e| e.into_inner()).clone()) })
    }

    /// Returns false for duplicate or late completion messages.
    pub fn finish_directory_import(&self, id: Uuid) -> bool {
        let mut imports = self.directory_imports.lock().unwrap_or_else(|error| error.into_inner());
        imports.active.remove(&id).is_some()
    }

    pub fn thumbnail_generations_running(&self) -> usize {
        self.thumbnails.state.lock().unwrap_or_else(|error| error.into_inner()).generations_running
    }

    pub(crate) fn begin_thumbnail_generation(self: &Arc<Self>, events: LibraryEventSender) -> ThumbnailActivity {
        self.thumbnails.state.lock().unwrap_or_else(|error| error.into_inner()).generations_running += 1;
        let _ = events.try_send(LibraryEvent::TransferStatusChanged);
        ThumbnailActivity(self.clone(), events)
    }

    /// Starts the one thumbnail batch; None while another batch runs.
    pub fn begin_thumbnail_work(self: &Arc<Self>, events: LibraryEventSender) -> Option<LibraryActivity> {
        {
            let mut state = self.thumbnails.state.lock().unwrap_or_else(|error| error.into_inner());
            if state.batch_active {
                return None;
            }
            state.batch_active = true;
            state.progress = ScanProgress::default();
        }
        let _ = events.try_send(LibraryEvent::TransferStatusChanged);
        Some(LibraryActivity { runtime: self.clone(), kind: LibraryActivityKind::ThumbnailWork, events })
    }

    /// Starts local file replay; None while another replay runs.
    pub fn begin_local_work(self: &Arc<Self>, events: LibraryEventSender) -> Option<LibraryActivity> {
        self.local_work_running.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok()?;
        let _ = events.try_send(LibraryEvent::TransferStatusChanged);
        Some(LibraryActivity { runtime: self.clone(), kind: LibraryActivityKind::LocalWork, events })
    }
}

#[derive(Clone, Copy)]
pub(crate) enum LibraryActivityKind {
    LocalWork,
    ThumbnailWork,
}

pub struct LibraryActivity {
    pub(crate) runtime: Arc<LibraryRuntime>,
    pub(crate) kind: LibraryActivityKind,
    pub(crate) events: LibraryEventSender,
}

impl Drop for LibraryActivity {
    fn drop(&mut self) {
        match self.kind {
            LibraryActivityKind::LocalWork => self.runtime.local_work_running.store(false, Ordering::Release),
            LibraryActivityKind::ThumbnailWork => {
                use crate::library::thumbnails::*;
                self.runtime.thumbnails.state.lock().unwrap_or_else(|error| error.into_inner()).batch_active = false;
            }
        }
        let _ = self.events.try_send(LibraryEvent::TransferStatusChanged);
    }
}

pub struct ThumbnailActivity(pub(crate) Arc<LibraryRuntime>, pub(crate) LibraryEventSender);

impl Drop for ThumbnailActivity {
    fn drop(&mut self) {
        ({
            use crate::library::thumbnails::*;
            let mut state = self.0.thumbnails.state.lock().unwrap_or_else(|error| error.into_inner());
            state.generations_running = state.generations_running.checked_sub(1).expect("thumbnail generation activity must be balanced");
        });
        let _ = self.1.try_send(LibraryEvent::TransferStatusChanged);
    }
}

#[cfg(test)]
mod import_progress_tests {
    use super::*;
    use library_model::ImportFileStage;

    #[test]
    fn file_progress_cannot_leak_into_the_next_import() {
        let runtime = LibraryRuntime::default();
        let first = Uuid::new_v4();
        runtime.begin_directory_import(first);
        runtime.advance_directory_import(first, 1, 0, 0);
        let old = runtime.start_import_file(first, "First.m4b".into(), Some(100));
        old.lock().unwrap().stage = ImportFileStage::Copying { copied_bytes: 42 };
        assert_eq!(runtime.directory_import_details().unwrap().current.unwrap().stage, ImportFileStage::Copying { copied_bytes: 42 });
        runtime.finish_directory_import(first);
        assert!(runtime.directory_import_details().is_none());
        let second = Uuid::new_v4();
        runtime.begin_directory_import(second);
        runtime.start_import_file(second, "Second.m4b".into(), None);
        old.lock().unwrap().stage = ImportFileStage::Saving;
        runtime.finish_directory_import(first);
        let current = runtime.directory_import_details().unwrap().current.unwrap();
        assert_eq!(current.name, "Second.m4b");
        assert_eq!(current.stage, ImportFileStage::Preparing);
        runtime.finish_import_file(second);
        assert!(runtime.directory_import_details().unwrap().current.is_none());
    }

    #[test]
    fn each_import_reports_its_own_current_file_and_counts() {
        let runtime = LibraryRuntime::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        runtime.begin_directory_import(first);
        runtime.begin_directory_import(second);
        runtime.advance_directory_import(first, 3, 1, 0);
        runtime.advance_directory_import(second, 1, 0, 0);
        let first_file = runtime.start_import_file(first, "First.m4b".into(), Some(100));
        first_file.lock().unwrap().stage = ImportFileStage::Copying { copied_bytes: 25 };
        let second_file = runtime.start_import_file(second, "Second.m4b".into(), Some(200));
        second_file.lock().unwrap().stage = ImportFileStage::Copying { copied_bytes: 150 };

        let first_progress = runtime.directory_import_details_for(Some(first)).unwrap();
        assert_eq!(first_progress.total, 3);
        assert_eq!(first_progress.succeeded, 1);
        assert_eq!(first_progress.current.unwrap().name, "First.m4b");
        let second_progress = runtime.directory_import_details_for(Some(second)).unwrap();
        assert_eq!(second_progress.total, 1);
        assert_eq!(second_progress.current.unwrap().name, "Second.m4b");
        assert!(!runtime.begin_directory_import(second), "duplicate registration preserves progress");
        assert!(runtime.finish_directory_import(first));
        assert!(!runtime.finish_directory_import(first));
        assert!(!runtime.advance_directory_import(first, 100, 100, 0));
        let aggregate = runtime.directory_import_details().unwrap();
        assert_eq!((aggregate.total, aggregate.succeeded), (4, 1), "completed imports remain in the overlapping group's totals");
        assert_eq!(aggregate.current.unwrap().name, "Second.m4b");
        runtime.finish_directory_import(second);
        assert!(runtime.directory_import_details().is_none());
    }
}
