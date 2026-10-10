//! Coordination state owned by one library session.

use std::cell::Cell;
use std::rc::Rc;

/// Mutable coordination state for one open library, independent of app policy
/// and of the storage implementation used by that library.
#[derive(Default)]
pub struct LibraryRuntime {
    local_work_running: Cell<bool>,
    thumbnail_work_running: Cell<bool>,
}

impl LibraryRuntime {

    /// Starts the one thumbnail batch; None while another batch runs.
    pub fn begin_thumbnail_work(self: &Rc<Self>) -> Option<LibraryActivity> {
        if self.thumbnail_work_running.replace(true) {
            return None;
        }
        Some(LibraryActivity { runtime: self.clone(), kind: LibraryActivityKind::ThumbnailWork })
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
