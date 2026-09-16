//! One native backend job owns the complete selected-directory import.
use super::{DirectoryImport, LibrarySession};
use crate::import_workflow::{Destinations, ProgressReporter};

impl LibrarySession {
    pub(crate) async fn import_selected_directory(&self, parent_id: crate::DirId, directory: DirectoryImport, create_root: bool) -> Result<Vec<crate::ImportFailure>, String> {
        let activity = self.directory_import_activity();
        let DirectoryImport { name, directories, mut files } = directory;
        let single_book = !create_root && directories.is_empty() && files.len() == 1;
        let prepared = self.prepare_directory_import(&parent_id, create_root.then_some(name), directories).await.map_err(|error| error.to_string())?;
        let parents = Destinations::new(prepared.directories);
        let mut failures = prepared.failures;
        files.retain(|file| crate::import_workflow::supported(&file.path));
        files.sort_by(|left, right| left.path.cmp(&right.path));
        let mut batch = DirectBatch { library: self, files: files.into_iter().map(Some).collect(), parents, single_book, activity: activity.id, progress: ProgressReporter::default(), succeeded: 0, failed: 0, failures: &mut failures };
        crate::import_workflow::run(&mut batch).await?;
        Ok(failures)
    }
}

struct DirectBatch<'a> {
    library: &'a LibrarySession,
    files: Vec<Option<crate::DirectoryImportFile>>,
    parents: Destinations,
    single_book: bool,
    activity: uuid::Uuid,
    progress: ProgressReporter,
    succeeded: u64,
    failed: u64,
    failures: &'a mut Vec<crate::ImportFailure>,
}
impl crate::import_workflow::Batch for DirectBatch<'_> {
    type Error = String;
    fn len(&self) -> usize {
        self.files.len()
    }
    async fn process(&mut self, index: usize) -> Result<(), String> {
        let file = self.files[index].take().expect("import item processed once");
        let result = async {
            let (parent_id, name) = self.parents.resolve(&file.path)?;
            let reader = file.source.open().await?;
            let result = if self.single_book { self.library.import_book_reader_direct(&parent_id, reader, name.to_owned()).await } else { self.library.import_book_reader(&parent_id, reader, name.to_owned()).await };
            result.map_err(|error| error.to_string())
        }
        .await;
        if result.is_ok() {
            self.succeeded += 1;
        } else {
            self.failed += 1;
        }
        if let Err(error) = result {
            let summary = crate::import_workflow::file_failure(&file.path, &error);
            self.failures.push(summary);
        }
        Ok(())
    }
    async fn progress(&mut self) -> Result<(), String> {
        if let Some(update) = self.progress.advance(self.files.len() as u64, self.succeeded, self.failed)? {
            self.library.advance_directory_import(self.activity, update.total, update.succeeded, update.failed);
        }
        Ok(())
    }
    async fn finish(&mut self) -> Result<(), String> {
        self.progress().await
    }
}
