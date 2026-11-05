//! Transfer browser-selected file capabilities to the storage worker.
use crate::{BackendError, DirId, DirectoryImport, ImportFailure, LibraryHandle};

pub(super) async fn import_directory(handle: &LibraryHandle, parent_id: DirId, directory: DirectoryImport, create_root: bool) -> Result<Vec<ImportFailure>, BackendError> {
    let files = js_sys::Array::new();
    let mut paths = Vec::with_capacity(directory.files.len());
    for file in directory.files {
        files.push(&file.source.into_file()?.into());
        paths.push(file.path);
    }
    let cleanup = handle.clone();
    let selected = client_platform_web::transport::import_io::send_selected_files(&files, move |token| cleanup.release_selected_files(token)).await?;
    handle.import_selected_files(parent_id, create_root, directory.name, directory.directories, paths, selected).await
}
