//! Prepared local Copy imports: flush bytes before one metadata transaction.
use crate::reader_import::{ImportMode, ReaderImportReport};
use crate::LibraryImport;
use crate::import::{validate_import_destination, ValidatedImport};
use client_platform_runtime::executor::run_blocking;
use client_runtime::BackendError;
use library_database::transfer::BookTransferState;
use library_database::{CreatedDirectory, ImportCommit};
use library_files::{asset_store::BookLease, AssetStore};
use library_model::RelativeBookPath;
use sync_common::{BookId, DirId};
use std::collections::{HashMap, HashSet};

pub(crate) struct ReaderImportBatch {
    pub assets: AssetStore,
    pub prepared_directories: HashSet<DirId>,
    imports: Vec<(ImportCommit, BookTransferState)>,
    pub sources: Vec<(String, u64, content_address::ContentHash)>,
    leases: Vec<BookLease>,
    directories: Vec<CreatedDirectory>,
    directory_paths: HashMap<DirId, String>,
}

impl ReaderImportBatch {
    pub fn new(importer: &LibraryImport, mode: ImportMode) -> Result<Option<Self>, BackendError> {
        if mode != ImportMode::Copy {
            return Ok(None);
        }
        Ok(Some(Self { assets: importer.assets.for_import_batch(), prepared_directories: HashSet::new(), imports: Vec::new(), sources: Vec::new(), leases: Vec::new(), directories: Vec::new(), directory_paths: HashMap::new() }))
    }

    pub async fn prepare_tree(&mut self, importer: &LibraryImport, parent: DirId, paths: &[Vec<String>]) -> Result<HashMap<Vec<String>, DirId>, BackendError> {
        let staged = importer.placement.stage_import_directories(parent, paths, &self.assets).await?;
        for directory in &staged.created {
            self.directory_paths.insert(directory.id, directory.relative_path.clone());
            self.prepared_directories.insert(directory.id);
        }
        self.prepared_directories.extend(staged.destinations.values().copied());
        self.directories.extend(staged.created);
        Ok(staged.destinations)
    }

    pub fn validate_destination(&self, importer: &LibraryImport, parent: DirId) -> Result<(), BackendError> {
        if self.directory_paths.contains_key(&parent) { return Ok(()); }
        validate_import_destination(&importer.database, &parent)
    }

    pub fn validate_import(&self, importer: &LibraryImport, parent: DirId, name: &str) -> Result<ValidatedImport, BackendError> {
        if self.directory_paths.contains_key(&parent) { return ValidatedImport::new(parent, name); }
        importer.validate_import(&parent, name)
    }

    pub fn native_book_path(&self, importer: &LibraryImport, parent: DirId, name: &str) -> Result<RelativeBookPath, BackendError> {
        match self.directory_paths.get(&parent) {
            Some(path) => RelativeBookPath::parse(&format!("/{path}/{name}")).map_err(BackendError::operation),
            None => importer.database.native_book_path(parent, name)?.ok_or_else(|| BackendError::message("import folder is unavailable")),
        }
    }

    pub async fn prepare_directory(&mut self, importer: &LibraryImport, parent: DirId) -> Result<(), BackendError> {
        if !self.prepared_directories.contains(&parent) {
            library_placement::prepare_directory(&importer.assets, &importer.database, parent).await?;
            self.prepared_directories.insert(parent);
        }
        Ok(())
    }

    pub fn push(&mut self, import: ImportCommit, state: BookTransferState, lease: BookLease) {
        self.imports.push((import, state));
        self.leases.push(lease);
    }

    pub fn destination_name(&self, parent: DirId, book: BookId) -> Option<&str> {
        self.imports.iter().find(|(import, _)| import.parent_id == parent && import.book_id == book).map(|(import, _)| import.published.name.as_str())
    }

    pub fn available_name(&self, parent: DirId, requested: &str, mut occupied: Vec<String>) -> String {
        occupied.extend(self.imports.iter().filter(|(import, _)| import.parent_id == parent).map(|(import, _)| import.published.name.clone()));
        library_replica::unique_file_name(requested, occupied.iter().map(String::as_str))
    }

    pub async fn finish(self, importer: &LibraryImport, report: &mut ReaderImportReport) -> Result<(), BackendError> {
        if self.imports.is_empty() && self.directories.is_empty() { return Ok(()); }
        let assets = self.assets.clone();
        run_blocking(move || assets.flush_import_batch()).await??;
        for directory in &self.directories {
            if self.assets.import_directory_id(&directory.relative_path)? != Some(directory.id) {
                return Err(BackendError::message("an import folder changed before the batch committed; retry the import"));
            }
        }
        // Preparation can span many books. Do not record stale observations
        // when a published file was edited or removed while finishing others.
        for (import, _) in &self.imports {
            let path = self.assets.native_library_root().join(import.published.relative_path.trim_start_matches('/'));
            if let Some(expected) = &import.published.fingerprint {
                if library_files::filesystem::fingerprint(&path)?.to_string() != *expected {
                    return Err(BackendError::message("a copied book changed before the import batch committed; retry the import"));
                }
            }
        }
        let created = importer.database.commit_inspected_transfer_batch_with_directories(self.imports, self.sources, self.directories)?;
        report.imported = created.iter().filter(|created| **created).count();
        report.already_present = created.len() - report.imported;
        // Leases stay alive through the file flush and the durable DB commit.
        drop(self.leases);
        importer.events.notify_contents_changed();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readest::{import_book_in_batch, ReadestBook, ReadestConfig};
    use crate::reader_transfer::test_importer;
    use serde_json::json;
    use std::fs;
    use sync_common::ROOT_DIR_ID;

    const FIRST: &[u8] = include_bytes!("../../book-inspection/tests/fixtures/back-matter-evidence.epub");
    const SECOND: &[u8] = include_bytes!("../../../tests/fixtures/cloud-cover.epub");

    async fn prepare(importer: &LibraryImport, root: &std::path::Path, name: &str, bytes: &[u8], batch: &mut ReaderImportBatch) {
        prepare_into(importer, root, ROOT_DIR_ID, name, bytes, batch).await;
    }

    async fn prepare_into(importer: &LibraryImport, root: &std::path::Path, parent: DirId, name: &str, bytes: &[u8], batch: &mut ReaderImportBatch) {
        let path = root.join(name);
        fs::write(&path, bytes).unwrap();
        let book: ReadestBook = serde_json::from_value(json!({"hash":name,"filePath":path,"format":"EPUB","title":name})).unwrap();
        import_book_in_batch(importer, root, parent, &book, &ReadestConfig::default(), ImportMode::Copy, Some(batch)).await.unwrap();
    }

    #[tokio::test]
    async fn failed_commit_rolls_back_folders_and_retry_recovers_the_same_numbered_tree() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fs::create_dir(destination.path().join("Readest")).unwrap();
        fs::write(destination.path().join("Readest/foreign"), b"keep").unwrap();
        let importer = test_importer(destination.path());
        let paths = vec![vec!["Readest".into(), "Shelf".into()]];
        let mut batch = ReaderImportBatch::new(&importer, ImportMode::Copy).unwrap().unwrap();
        let parent = batch.prepare_tree(&importer, ROOT_DIR_ID, &paths).await.unwrap()[&paths[0]];
        prepare_into(&importer, source.path(), parent, "book.epub", FIRST, &mut batch).await;
        assert_eq!(importer.database.book_count().unwrap(), 0);
        assert!(importer.database.child_directory(&ROOT_DIR_ID, "Readest 2").unwrap().is_none());
        assert!(destination.path().join("Readest 2/Shelf/book.epub").is_file());
        batch.imports[0].0.parent_id = DirId::new_v4();
        assert!(batch.finish(&importer, &mut ReaderImportReport::default()).await.is_err());
        assert_eq!(importer.database.book_count().unwrap(), 0);
        assert!(importer.database.child_directory(&ROOT_DIR_ID, "Readest 2").unwrap().is_none());
        let mut retry = ReaderImportBatch::new(&importer, ImportMode::Copy).unwrap().unwrap();
        let recovered = retry.prepare_tree(&importer, ROOT_DIR_ID, &paths).await.unwrap()[&paths[0]];
        assert_eq!(parent, recovered);
        prepare_into(&importer, source.path(), recovered, "book.epub", FIRST, &mut retry).await;
        let mut report = ReaderImportReport::default();
        retry.finish(&importer, &mut report).await.unwrap();
        assert_eq!(report.imported, 1);
        assert!(importer.database.is_live_destination(&recovered).unwrap());
        assert_eq!(fs::read(source.path().join("book.epub")).unwrap(), FIRST);
        assert_eq!(fs::read(destination.path().join("Readest/foreign")).unwrap(), b"keep");
        assert!(!destination.path().join("Readest 3").exists());
        assert_eq!(fs::read_dir(destination.path().join("Readest 2/Shelf")).unwrap().filter_map(Result::ok).filter(|entry| entry.path().extension().is_some_and(|ext| ext == "epub")).count(), 1);
    }

    #[tokio::test]
    async fn changed_folder_marker_prevents_folders_and_books_from_committing() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let importer = test_importer(destination.path());
        let paths = vec![vec!["Readest".into()]];
        let mut batch = ReaderImportBatch::new(&importer, ImportMode::Copy).unwrap().unwrap();
        let parent = batch.prepare_tree(&importer, ROOT_DIR_ID, &paths).await.unwrap()[&paths[0]];
        prepare_into(&importer, source.path(), parent, "book.epub", FIRST, &mut batch).await;
        fs::write(destination.path().join("Readest/.biblos_uuid"), DirId::new_v4().to_string()).unwrap();
        assert!(batch.finish(&importer, &mut ReaderImportReport::default()).await.is_err());
        assert!(importer.database.child_directory(&ROOT_DIR_ID, "Readest").unwrap().is_none());
        assert_eq!(importer.database.book_count().unwrap(), 0);
        assert_eq!(fs::read(source.path().join("book.epub")).unwrap(), FIRST);
    }

    #[tokio::test]
    async fn batch_defers_metadata_reuses_duplicates_and_handles_portable_name_collisions() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let importer = test_importer(destination.path());
        let mut batch = ReaderImportBatch::new(&importer, ImportMode::Copy).unwrap().unwrap();
        prepare(&importer, source.path(), "Book.epub", FIRST, &mut batch).await;
        prepare(&importer, source.path(), "duplicate.epub", FIRST, &mut batch).await;
        prepare(&importer, source.path(), "book.epub", SECOND, &mut batch).await;
        assert_eq!(importer.database.book_count().unwrap(), 0);
        assert!(!destination.path().join("duplicate.epub").exists());
        assert_ne!(batch.imports[0].0.published.name.to_lowercase(), batch.imports[2].0.published.name.to_lowercase());
        let mut report = ReaderImportReport::default();
        batch.finish(&importer, &mut report).await.unwrap();
        assert_eq!((report.imported, report.already_present), (2, 1));
        assert_eq!(importer.database.book_count().unwrap(), 2);
        assert_eq!(fs::read(source.path().join("Book.epub")).unwrap(), FIRST);
        assert_eq!(fs::read(source.path().join("book.epub")).unwrap(), SECOND);
    }

    #[tokio::test]
    async fn failed_batch_commit_rolls_back_all_books_and_can_be_retried() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let importer = test_importer(destination.path());
        let mut batch = ReaderImportBatch::new(&importer, ImportMode::Copy).unwrap().unwrap();
        prepare(&importer, source.path(), "first.epub", FIRST, &mut batch).await;
        prepare(&importer, source.path(), "second.epub", SECOND, &mut batch).await;
        batch.imports[1].0.parent_id = DirId::new_v4();
        assert!(batch.finish(&importer, &mut ReaderImportReport::default()).await.is_err());
        assert_eq!(importer.database.book_count().unwrap(), 0);
        let source_path = source.path().join("first.epub");
        assert!(importer.database.cached_import_source(source_path.to_str().unwrap(), library_files::filesystem::fingerprint(&source_path).unwrap()).unwrap().is_none());
        let mut retry = ReaderImportBatch::new(&importer, ImportMode::Copy).unwrap().unwrap();
        prepare(&importer, source.path(), "first.epub", FIRST, &mut retry).await;
        prepare(&importer, source.path(), "second.epub", SECOND, &mut retry).await;
        retry.finish(&importer, &mut ReaderImportReport::default()).await.unwrap();
        assert_eq!(importer.database.book_count().unwrap(), 2);
    }

    #[tokio::test]
    async fn source_revision_cache_invalidates_edits_even_with_restored_mtime() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let importer = test_importer(destination.path());
        let path = source.path().join("book.epub");
        let book: ReadestBook = serde_json::from_value(json!({"hash":"cached","filePath":path,"format":"EPUB","title":"Book"})).unwrap();
        let mut batch = ReaderImportBatch::new(&importer, ImportMode::Copy).unwrap().unwrap();
        prepare(&importer, source.path(), "book.epub", FIRST, &mut batch).await;
        batch.finish(&importer, &mut ReaderImportReport::default()).await.unwrap();
        let revision = library_files::filesystem::fingerprint(&path).unwrap();
        assert_eq!(importer.database.cached_import_source(path.to_str().unwrap(), revision).unwrap(), Some(blake3::hash(FIRST).into()));
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&path, SECOND).unwrap();
        fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
        let next = library_files::filesystem::fingerprint(&path).unwrap();
        assert_ne!(revision, next);
        assert!(importer.database.cached_import_source(path.to_str().unwrap(), next).unwrap().is_none());
        let mut retry = ReaderImportBatch::new(&importer, ImportMode::Copy).unwrap().unwrap();
        import_book_in_batch(&importer, source.path(), ROOT_DIR_ID, &book, &ReadestConfig::default(), ImportMode::Copy, Some(&mut retry)).await.unwrap();
        retry.finish(&importer, &mut ReaderImportReport::default()).await.unwrap();
        assert_eq!(importer.database.book_count().unwrap(), 2);
        assert_eq!(importer.database.cached_import_source(path.to_str().unwrap(), next).unwrap(), Some(blake3::hash(SECOND).into()));
    }

    #[tokio::test]
    async fn cached_sources_merge_new_notes_and_repair_a_changed_destination() {
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let importer = test_importer(destination.path());
        let mut batch = ReaderImportBatch::new(&importer, ImportMode::Copy).unwrap().unwrap();
        prepare(&importer, source.path(), "book.epub", FIRST, &mut batch).await;
        batch.finish(&importer, &mut ReaderImportReport::default()).await.unwrap();
        let path = source.path().join("book.epub");
        let book: ReadestBook = serde_json::from_value(json!({"hash":"cached","filePath":path,"format":"EPUB","title":"Book"})).unwrap();
        let config: ReadestConfig = serde_json::from_value(json!({"booknotes":[{"id":"new","type":"annotation","cfi":"epubcfi(/6/4!/4/2,/1:0,/1:1)","text":"A","note":"New note","createdAt":1}]})).unwrap();
        let mut repeat = ReaderImportBatch::new(&importer, ImportMode::Copy).unwrap().unwrap();
        let result = import_book_in_batch(&importer, source.path(), ROOT_DIR_ID, &book, &config, ImportMode::Copy, Some(&mut repeat)).await.unwrap();
        repeat.finish(&importer, &mut ReaderImportReport::default()).await.unwrap();
        assert_eq!(importer.database.annotations(result.book_id).unwrap().len(), 1);
        let published = destination.path().join("book.epub");
        let modified = fs::metadata(&published).unwrap().modified().unwrap();
        fs::write(&published, vec![b'x'; FIRST.len()]).unwrap();
        fs::File::options().write(true).open(&published).unwrap().set_modified(modified).unwrap();
        let mut repair = ReaderImportBatch::new(&importer, ImportMode::Copy).unwrap().unwrap();
        import_book_in_batch(&importer, source.path(), ROOT_DIR_ID, &book, &config, ImportMode::Copy, Some(&mut repair)).await.unwrap();
        let mut report = ReaderImportReport::default();
        repair.finish(&importer, &mut report).await.unwrap();
        assert_eq!(report.already_present, 1);
        let repaired = importer.database.book_paths(&result.book_id).unwrap();
        assert_eq!(fs::read(destination.path().join(repaired[0].as_str().trim_start_matches('/'))).unwrap(), FIRST);
        assert_eq!(importer.database.book_count().unwrap(), 1);
    }

    #[tokio::test]
    async fn missing_or_changed_published_file_prevents_the_batch_commit() {
        for remove in [false, true] {
            let source = tempfile::tempdir().unwrap();
            let destination = tempfile::tempdir().unwrap();
            let importer = test_importer(destination.path());
            let mut batch = ReaderImportBatch::new(&importer, ImportMode::Copy).unwrap().unwrap();
            prepare(&importer, source.path(), "book.epub", FIRST, &mut batch).await;
            let path = destination.path().join("book.epub");
            if remove { fs::remove_file(path).unwrap(); } else { fs::write(path, b"changed").unwrap(); }
            assert!(batch.finish(&importer, &mut ReaderImportReport::default()).await.is_err());
            assert_eq!(importer.database.book_count().unwrap(), 0);
            assert_eq!(fs::read(source.path().join("book.epub")).unwrap(), FIRST);
        }
    }
}
