//! Connect the shared catalog service to open library views.
use crate::executor::BackendExecutor;
use crate::management::LibraryService;
use std::path::Path;
use std::sync::{Arc, Weak};

/// Start one catalog service shared by every library in this application.
pub fn start(app_data_dir: &Path, executor: &BackendExecutor, libraries: &Arc<LibraryService>) {
    let state: Weak<LibraryService> = Arc::downgrade(libraries);
    library_catalog::start(app_data_dir, executor, move || {
        if let Some(state) = state.upgrade() {
            for library in state.open_handles() {
                library.refresh_catalog();
            }
        }
    });
}
