//! Browser selected-folder imports: each file is opened from its source,
//! staged in browser storage, and finished through the shared import path.

use std::collections::HashMap;
use std::time::Duration;

use client_platform_runtime::executor::sleep;
use client_runtime::BackendError;
use sync_common::DirId;

use crate::import_workflow::Destinations;
use crate::{DirectoryImport, ImportFailure, LibraryImport};

impl LibraryImport {
    pub async fn import_selected_directory(&self, parent_id: DirId, directory: DirectoryImport, create_root: bool, cancelled: &dyn Fn() -> bool) -> Result<Vec<ImportFailure>, BackendError> {
        if cancelled() {
            return Err(BackendError::message("directory import cancelled"));
        }
        let DirectoryImport { name, directories, mut files } = directory;
        let prepared = self.prepare_directory_import(&parent_id, create_root.then_some(name), directories).await?;
        let parents = Destinations::new(prepared.directories);
        let mut failures = prepared.failures;
        files.retain(|file| crate::import_workflow::supported(&file.path));
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let mut batch = DirectBatch { cancelled, library: self, files: files.into_iter().map(Some).collect(), parents, failures: &mut failures };
        crate::import_workflow::run(&mut batch).await?;
        Ok(failures)
    }
}

struct DirectBatch<'a> {
    cancelled: &'a dyn Fn() -> bool,
    library: &'a LibraryImport,
    files: Vec<Option<crate::DirectoryImportFile>>,
    parents: Destinations,
    failures: &'a mut Vec<ImportFailure>,
}
impl crate::import_workflow::Batch for DirectBatch<'_> {
    type Error = BackendError;
    fn len(&self) -> usize {
        self.files.len()
    }
    async fn process(&mut self, index: usize) -> Result<(), BackendError> {
        if (self.cancelled)() {
            return Err(BackendError::message("directory import cancelled"));
        }
        let file = self.files.get_mut(index).and_then(Option::take).ok_or_else(|| BackendError::message("import item is unavailable or already processed"))?;
        let result = async {
            let (parent_id, name) = self.parents.resolve(&file.path)?;
            // A book already at this name was imported before: continuing an
            // import skips it without reading the file.
            if self.library.database.has_book_named(&parent_id, name).map_err(BackendError::operation)? {
                return Ok(());
            }
            let import = self.library.validate_import(&parent_id, name)?;
            // Browser storage is keyed by content, so the name needs no collision check.
            let staged = self.library.assets.stage_browser_file(&file.source.into_file()?).await.map_err(BackendError::operation)?;
            self.library.finish_import(import, staged).await.map(drop)
        }
        .await;
        if let Err(error) = result {
            let summary = crate::import_workflow::file_failure(&file.path, &error);
            self.failures.push(summary);
        }
        Ok(())
    }
}

impl LibraryImport {
    pub(crate) async fn prepare_directory_import(&self, parent_id: &DirId, name: Option<String>, mut directories: Vec<Vec<String>>) -> Result<crate::PreparedDirectoryImport, BackendError> {
        crate::import::validate_import_destination(&self.database, parent_id)?;
        // A folder of the same name in the same place is reused, so importing a
        // folder again continues it instead of creating a second copy.
        let create = |parent: &DirId, name: &str| -> Result<DirId, BackendError> {
            if let Some(existing) = self.database.child_directory(parent, name.trim()).map_err(BackendError::operation)? {
                return Ok(existing);
            }
            Ok(self.placement.create_directory(parent, name)?.id)
        };
        // Let queued import status reach subscribers before doing SQLite work;
        // large directory trees must not monopolize the executor.
        sleep(Duration::ZERO).await.map_err(BackendError::operation)?;
        let root_id = match name {
            Some(name) => create(parent_id, &name)?,
            None => *parent_id,
        };
        let mut imported_directories = HashMap::from([(Vec::<String>::new(), root_id)]);
        let mut failures = Vec::new();

        // A parent path sorts before every path that extends it.
        directories.sort_unstable();
        directories.dedup();
        let mut slice_started = web_time::Instant::now();
        for path in directories {
            if slice_started.elapsed() >= Duration::from_millis(8) {
                sleep(Duration::ZERO).await.map_err(BackendError::operation)?;
                slice_started = web_time::Instant::now();
            }
            let Some((name, parent_path)) = path.split_last() else { continue };
            let Some(parent_id) = imported_directories.get(parent_path) else {
                failures.push(crate::import_workflow::folder_failure(&path, "parent folder could not be imported"));
                continue;
            };
            match create(parent_id, name) {
                Ok(directory) => {
                    imported_directories.insert(path, directory);
                }
                Err(error) => failures.push(crate::import_workflow::folder_failure(&path, &error)),
            }
        }

        Ok(crate::PreparedDirectoryImport { directories: imported_directories.into_iter().collect(), failures })
    }
}
