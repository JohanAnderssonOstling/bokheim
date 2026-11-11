//! Local content identity, publication leases, and commit preparation.

use crate::LibraryImport;
use crate::import::ValidatedImport;
use book_inspection::{InspectedBook, PdfSourceIdentity};
use client_protocol::BackendError;
use content_address::ContentHash;
use library_database::transfer::{BookTransferState, ImportedBook};
use library_database::{ContributorNameMatcher, ImportCommit};
use library_files::asset_store::{BookLease, StagedBook};
use library_model::PublishedStagedBook;
use sync_common::BookId;

#[derive(Clone, Copy)]
pub(crate) struct ImportContent {
    pub book_id: BookId,
    pub byte_checksum: Option<ContentHash>,
    pub audiobook_content_hash: Option<ContentHash>,
    pub size_bytes: u64,
    pub source_identity: Option<PdfSourceIdentity>,
}

impl LibraryImport<'_> {
    pub(crate) async fn resolve_import_content(&self, byte_checksum: ContentHash, size_bytes: u64, source_identity: Option<PdfSourceIdentity>) -> Result<ImportContent, BackendError> {
        let book_id = self.storage.database().resolve_scanned_content(byte_checksum, source_identity).await.map_err(BackendError::operation)?;
        Ok(ImportContent { book_id, byte_checksum: Some(byte_checksum), audiobook_content_hash: None, size_bytes, source_identity })
    }

    pub(crate) async fn resolve_audiobook_import(&self, checksum: ContentHash, byte_checksum: Option<ContentHash>, size_bytes: u64, known: Option<BookId>) -> Result<ImportContent, BackendError> {
        let known = match (known, byte_checksum) {
            (Some(known), _) => Some(known),
            (None, Some(bytes)) => Some(self.storage.database().resolve_scanned_content(bytes, None).await?),
            (None, None) => None,
        };
        let book_id = self.storage.database().resolve_audiobook_content(checksum, known).await?;
        Ok(ImportContent { book_id, byte_checksum, audiobook_content_hash: Some(checksum), size_bytes, source_identity: None })
    }

    pub(crate) async fn staged_audiobook_content(&self, staged: &StagedBook) -> Result<ContentHash, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        let reader = staged.open_reader().map_err(BackendError::operation)?;
        #[cfg(target_arch = "wasm32")]
        let reader = staged.open_reader().await.map_err(BackendError::operation)?;
        let checksum = cpu_host::submit_book(&*self.cpu, cpu_host::HashAudiobookContent {}, cpu_host::as_cpu_reader(reader)).await?;
        checksum.content_hash().ok_or_else(|| BackendError::message("audiobook fingerprint is unavailable"))
    }

    /// Hold local bytes through publication and the durable metadata commit.
    /// Cloud admission belongs to the upload worker, after this commit.
    pub(crate) async fn lease_import_content(&self, content: &ImportContent) -> Result<BookLease, BackendError> {
        self.storage.assets().lease_book(&content.book_id).await.map_err(|error| BackendError::operation(error).context("could not lock the imported book"))
    }

    pub(crate) async fn prepare_import_commit(&self, import: ValidatedImport, content: ImportContent, published: PublishedStagedBook, mut inspection: Option<InspectedBook>, inspection_version: i64) -> Result<ImportCommit, BackendError> {
        if let Some(pdf) = inspection.as_mut().and_then(|inspection| inspection.pdf.as_mut()) {
            pdf.book_id = content.book_id.to_hex();
        }
        let request_thumbnail = !self.storage.assets().thumbnail_set_exists(&content.book_id).await.map_err(BackendError::operation)?;
        let restore_paths = self.import_restore_paths(&content.book_id).await?;
        Ok(ImportCommit {
            source_identity: content.source_identity,
            byte_checksum: content.byte_checksum,
            audiobook_content_hash: content.audiobook_content_hash,
            parent_id: import.parent_id,
            file_name: import.file_name,
            format: import.format,
            book_id: content.book_id,
            size_bytes: content.size_bytes,
            inspection_version,
            inspection,
            published,
            request_thumbnail,
            restore_paths,
        })
    }

    /// Resolve fallible commit data before the transaction, then announce its success.
    pub(crate) async fn commit_import(&self, import: ValidatedImport, content: ImportContent, inspection: Option<InspectedBook>, published: PublishedStagedBook) -> Result<(), BackendError> {
        let version = book_inspection::inspection_version(import.format);
        let commit = self.prepare_import_commit(import, content, published, inspection, version).await?;
        let catalog_record = match content.byte_checksum {
            Some(checksum) => self.storage.database().catalog_import_record(BookId::from(checksum), commit.format).await.map_err(BackendError::operation)?,
            None => None,
        };
        self.storage.database().commit_import_with_catalog_answer(commit, Some(ContributorNameMatcher::Normalized), catalog_record).await.map_err(BackendError::operation)?;
        self.events.notify_contents_changed();
        Ok(())
    }

    /// The caller retargets reader state before publishing or queuing a batch.
    pub(crate) async fn commit_transferred_import(&self, commit: ImportCommit, state: BookTransferState, inspected: bool) -> Result<ImportedBook, BackendError> {
        let book_id = commit.book_id;
        let created = if inspected { self.storage.database().commit_inspected_transfer_import(commit, state).await } else { self.storage.database().commit_transfer_import(commit, state).await }.map_err(BackendError::operation)?;
        self.events.notify_contents_changed();
        Ok(ImportedBook { book_id, created })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::reader_transfer::test_importer;
    use book_model::BookFormat;
    use std::fs::{self, File};
    use sync_common::ROOT_DIR_ID;

    const EPUB: &[u8] = include_bytes!("../../book-inspection/tests/fixtures/back-matter-evidence.epub");

    #[tokio::test]
    async fn stored_dropped_transferred_and_moved_books_share_identity_and_metadata() {
        let root = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let path = source.path().join("book.epub");
        fs::write(&path, EPUB).unwrap();
        let importer_placement = test_importer(root.path()).await;
        let importer_storage = importer_placement.write().await;
        let importer = LibraryImport::new(&importer_storage, std::sync::Arc::new(cpu_host::NativeHost));
        let identity = importer.import_stored_book(ROOT_DIR_ID, "book.epub".into(), path.clone()).await.unwrap();
        let original = importer.storage.database().book_transfer_state(identity).await.unwrap();

        let failures = importer.import_paths(ROOT_DIR_ID, vec![path.clone()], false).await.unwrap();
        assert!(failures.is_empty(), "{failures:?}");
        assert!(path.exists(), "copying must preserve the source");
        let transferred = importer.import_transferred_book(ROOT_DIR_ID, "book.epub".into(), Box::new(File::open(&path).unwrap()), fs::metadata(&path).unwrap().len(), original.clone()).await.unwrap();
        assert_eq!(transferred.book_id, identity);
        assert!(!transferred.created);
        let moved = importer.move_book_from_path(ROOT_DIR_ID, &path).await.unwrap();
        assert_eq!(moved.book_id, identity);
        assert!(!moved.created);
        assert!(!path.exists());

        assert_eq!(importer.storage.database().book_count().unwrap(), 1);
        let paths = importer.storage.database().book_paths(&identity).await.unwrap();
        assert!(importer.storage.assets().book_exists(&identity, &paths).unwrap());
        let current = importer.storage.database().book_transfer_state(identity).await.unwrap();
        assert_eq!(current.addition.record.title, original.addition.record.title);
        assert_eq!(current.expected_checksum(), original.expected_checksum());
        assert!(importer.storage.database().has_import_inspection(&identity, BookFormat::Epub, book_inspection::inspection_version(BookFormat::Epub)).unwrap());
    }
}
