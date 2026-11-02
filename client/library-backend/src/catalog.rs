//! Connect the shared catalog service to open library views.
use crate::executor::BackendExecutor;
use crate::library::LibraryDirectory;
use std::path::Path;
use std::sync::Arc;

/// Start one catalog service shared by every library in this application.
pub fn start(app_data_dir: &Path, executor: &BackendExecutor, libraries: Arc<LibraryDirectory>) {
    library_catalog::start(app_data_dir, executor, move || match libraries.all() {
        Ok(libraries) => {
            for library in libraries {
                library.refresh_catalog();
            }
        }
        Err(error) => log::warn!("catalog views could not be refreshed: {error}"),
    });
}
