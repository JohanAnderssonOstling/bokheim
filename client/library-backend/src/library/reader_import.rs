//! Reader import jobs retain cancellation and progress in their owning session.

use std::cell::Cell;
use std::rc::Rc;

use library_import::reader_import::{ReaderImportOptions, ReaderImportProgress, ReaderImportReport, ReaderImportSource, ReaderLibrary, ReaderLibrarySummary};
use uuid::Uuid;

use super::{job_registry::JobGuard, LibrarySession};
use crate::BackendError;

pub(crate) type ReaderJobGuard = JobGuard<ReaderImportProgress, Rc<Cell<bool>>>;

impl LibrarySession {
    pub(crate) async fn reader_import_summary(&self, source: ReaderImportSource) -> Result<ReaderLibrarySummary, BackendError> {
        let mut summary = ReaderLibrary::load_default(source).await?.summary();
        summary.can_move = true;
        Ok(summary)
    }

    pub(crate) fn prepare_reader_import(&self, id: Uuid) -> Result<ReaderJobGuard, BackendError> {
        self.runtime.reader_imports.prepare(id, ReaderImportProgress::default(), Rc::new(Cell::new(false)), "this reader import is already active")
    }

    pub(crate) fn reader_import_progress(&self, id: Uuid) -> Option<ReaderImportProgress> {
        self.runtime.reader_imports.progress(id)
    }

    pub(crate) fn cancel_reader_import(&self, id: Uuid) -> bool {
        self.runtime.reader_imports.cancel(id)
    }

    pub(crate) async fn import_reader(&self, source: ReaderImportSource, options: ReaderImportOptions, job: ReaderJobGuard) -> Result<ReaderImportReport, BackendError> {
        // Cancellation can arrive while this job waits for import admission.
        if job.control.get() {
            return Ok(ReaderImportReport { cancelled: true, ..Default::default() });
        }
        let source = ReaderLibrary::load_default(source).await?;
        let _activity = self.directory_import_activity();
        source
            .import(&self.import, options, &|| job.control.get(), |progress| {
                job.set_progress(progress);
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{
        commands::{prepare_library_request, PreparedRequest},
        handle::concurrency_tests::fixture,
        LibraryCommand, PendingCommand, ReadyCommand,
    };
    use library_import::reader_import::ImportMode;
    use sync_common::ROOT_DIR_ID;

    #[tokio::test]
    async fn reader_cancellation_is_registered_before_admission_and_cleans_up() {
        for source in [ReaderImportSource::Readest, ReaderImportSource::Foliate] {
            let root = tempfile::tempdir().unwrap();
            let (mut actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
            let import_id = Uuid::new_v4();
            let options = ReaderImportOptions { parent_id: ROOT_DIR_ID, group: None, mode: ImportMode::Move };
            let operation = prepare_library_request(&mut actor, LibraryCommand::Pending(PendingCommand::ImportReader { import_id, source, options })).unwrap();
            assert!(actor.session.reader_import_progress(import_id).is_some());
            assert!(actor.session.prepare_reader_import(import_id).is_err());
            let cancel = prepare_library_request(&mut actor, LibraryCommand::Ready(ReadyCommand::CancelReaderImport { import_id })).unwrap();
            let PreparedRequest::Ready(reply) = cancel else { panic!("cancellation must not wait for admission") };
            assert!(reply.take::<bool>().unwrap());
            let PreparedRequest::Pending(operation) = operation else { panic!("import must be asynchronous") };
            let report = operation.future.await.unwrap().take::<ReaderImportReport>().unwrap();
            assert!(report.cancelled);
            assert!(actor.session.reader_import_progress(import_id).is_none());
            assert!(!root.path().join(source.name()).exists());
        }
    }
    #[tokio::test]
    async fn opening_library_recovers_an_abandoned_legacy_claim_before_scanning() {
        use serde_json::json;
        use std::fs;
        let root = tempfile::tempdir().unwrap();
        let original = tempfile::tempdir().unwrap();
        let source = original.path().join("book.epub");
        let journal = root.path().join("Nested/.bokheim-import-move-restart-test");
        fs::create_dir_all(&journal).unwrap();
        let staged = journal.join("book.epub");
        fs::write(&staged, b"unpublished book").unwrap();
        fs::write(journal.join("recovery.json"), serde_json::to_vec(&json!({"source":source,"staged":staged,"destination":null})).unwrap()).unwrap();
        let (_actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
        assert_eq!(fs::read(&source).unwrap(), b"unpublished book");
        assert!(!journal.exists());
    }

    #[tokio::test]
    async fn reopening_library_keeps_a_committed_move_when_cleanup_was_interrupted() {
        use library_files::import_move::ImportMove;
        use std::fs;
        let root = tempfile::tempdir().unwrap();
        let original = tempfile::tempdir().unwrap();
        let source = original.path().join("book.epub");
        fs::write(&source, include_bytes!("../../tests/fixtures/cloud-cover.epub")).unwrap();
        let (actor, handle, inbox) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
        let moved = actor.session.import.move_book_from_path(ROOT_DIR_ID, &source).await.unwrap();
        let name = actor.session.db.transfer_destination_name(&moved.book_id, &ROOT_DIR_ID).unwrap().unwrap();
        let destination = root.path().join(name).canonicalize().unwrap();
        // Recreate the committed publication with a real indexed claim, then
        // leave its journal as if the process exited before private cleanup.
        fs::rename(&destination, &source).unwrap();
        let mut claim = ImportMove::claim_in_library(&source, root.path(), root.path()).unwrap();
        let journal = claim.path().parent().unwrap().to_owned();
        assert_eq!(claim.publish(root.path(), &[]).unwrap(), destination);
        claim.mark_committed();
        drop(claim);
        drop(actor);
        drop(handle);
        drop(inbox);
        let (actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
        assert!(!source.exists());
        assert!(!journal.exists());
        assert!(destination.is_file());
        assert_eq!(actor.session.db.book_count().unwrap(), 1);
    }
}
