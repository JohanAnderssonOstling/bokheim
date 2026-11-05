//! Default Readest discovery, group selection and whole-library imports.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use client_platform_runtime::executor::run_blocking;
use client_protocol::BackendError;
use library_replica::filenames;
use sync_common::DirId;

use super::{ImportMode, ReadestBook, ReadestConfig, import_book_in_batch};
use crate::LibraryImport;
use crate::reader_batch::ReaderImportBatch;
use crate::reader_import::{ReaderImportBook, ReaderImportSource, import_books};
use crate::reader_transfer::ReaderBookResult;

pub use crate::reader_import::ReaderBookCounts as ReadestBookCounts;
pub use crate::reader_import::ReaderGroup as ReadestGroup;
pub use crate::reader_import::ReaderImportOptions as ReadestImportOptions;
pub use crate::reader_import::ReaderImportReport as ReadestImportReport;
pub use crate::reader_import::ReaderLibrarySummary as ReadestLibrarySummary;

impl ReadestBookCounts {
    fn include(&mut self, book: &ReadestBook) {
        match book.format.as_str() {
            "EPUB" => self.epub += 1,
            "PDF" => self.pdf += 1,
            _ => {}
        }
    }
}

pub struct ReadestLibrary {
    directory: PathBuf,
    books: Vec<ReadestBook>,
}

impl ReadestLibrary {
    /// Prefer the native configured library, then the default Flatpak/Snap
    /// locations. Import selects one library; export may target several profiles.
    pub fn default_directory() -> Result<PathBuf, BackendError> {
        directory_from_profiles(reader_interop::profiles::readest_directories()?)
    }

    pub async fn load_default() -> Result<Self, BackendError> {
        run_blocking(Self::load_default_sync).await?
    }

    pub(crate) fn load_default_sync() -> Result<Self, BackendError> {
        Self::open(Self::default_directory()?)
    }

    /// Explicit paths are for backend integration and isolated fixture tests;
    /// the application always uses the default location.
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, BackendError> {
        let directory = directory.as_ref().to_owned();
        let path = directory.join("library.json");
        let bytes = fs::read(&path).map_err(|error| BackendError::operation(error).context("could not read the default Readest library"))?;
        let entries: Vec<serde_json::Value> = serde_json::from_slice(&bytes).map_err(BackendError::operation)?;
        let books: Vec<ReadestBook> =
            entries.into_iter().filter(|book| book["deletedAt"].is_null() && matches!(book["format"].as_str(), Some("EPUB" | "PDF"))).map(serde_json::from_value).collect::<Result<Vec<_>, _>>().map_err(BackendError::operation)?;
        Ok(Self { directory, books })
    }

    pub fn summary(&self) -> ReadestLibrarySummary {
        let mut counts = ReadestBookCounts::default();
        let mut groups = BTreeMap::<String, ReadestBookCounts>::new();
        for book in &self.books {
            counts.include(book);
            let mut path = String::new();
            for component in book.group_name.as_deref().unwrap_or_default().split('/') {
                if !path.is_empty() {
                    path.push('/');
                }
                path.push_str(component);
                groups.entry(path.clone()).or_default().include(book);
            }
        }
        ReadestLibrarySummary { counts, groups: groups.into_iter().map(|(name, counts)| ReadestGroup { name, counts }).collect(), can_move: true }
    }

    pub async fn import(self, importer: &LibraryImport<'_>, options: ReadestImportOptions) -> Result<ReadestImportReport, BackendError> {
        let Self { directory, books } = self;
        let books = books.into_iter().filter(|book| group_matches(book.group_name.as_deref().unwrap_or_default(), options.group.as_deref())).map(|book| ReadestImportBook { directory: &directory, book }).collect();
        import_books(importer, books, options).await
    }
}

fn configured_directory(data: &Path, config: &Path) -> Result<PathBuf, BackendError> {
    let settings = match fs::read(config.join("settings.json")) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    reader_interop::profiles::readest_books_directory(data, settings.as_deref()).map_err(BackendError::operation)
}

fn directory_from_profiles(profiles: Vec<(PathBuf, PathBuf)>) -> Result<PathBuf, BackendError> {
    for (data, config) in profiles {
        let directory = configured_directory(&data, &config)?;
        if directory.join("library.json").is_file() {
            return Ok(directory);
        }
    }
    Err(BackendError::message("could not find Readest's library in its default locations"))
}

struct ReadestImportBook<'a> {
    directory: &'a Path,
    book: ReadestBook,
}

impl ReaderImportBook for ReadestImportBook<'_> {
    const SOURCE: ReaderImportSource = ReaderImportSource::Readest;

    fn title(&self) -> &str {
        &self.book.title
    }

    fn folders(&self) -> Result<Vec<String>, BackendError> {
        group_components(self.book.group_name.as_deref().unwrap_or_default())
    }

    async fn import(self, importer: &LibraryImport<'_>, parent: DirId, mode: ImportMode, batch: Option<&mut ReaderImportBatch>) -> Result<ReaderBookResult, BackendError> {
        // Validate the hash before using it as a config path, including books
        // whose source lives outside Readest's managed directory.
        let hash = filenames::validate_component(&self.book.hash)?;
        let path = self.directory.join(hash).join("config.json");
        let config = run_blocking(move || -> Result<ReadestConfig, BackendError> {
            match fs::read(path) {
                Ok(bytes) => serde_json::from_slice(&bytes).map_err(BackendError::operation),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ReadestConfig::default()),
                Err(error) => Err(error.into()),
            }
        })
        .await??;
        import_book_in_batch(importer, self.directory, parent, &self.book, &config, mode, batch).await
    }
}

fn group_matches(name: &str, selected: Option<&str>) -> bool {
    match selected {
        None => true,
        Some("") => name.is_empty(),
        Some(selected) => name == selected || name.strip_prefix(selected).is_some_and(|rest| rest.starts_with('/')),
    }
}

fn group_components(group: &str) -> Result<Vec<String>, BackendError> {
    let mut components = vec!["Readest".to_owned()];
    if !group.is_empty() {
        for component in group.split('/') {
            let component = filenames::validate_component(component)?;
            if component.starts_with('.') {
                return Err(BackendError::message("dot-prefixed library directories are unsupported"));
            }
            components.push(component);
        }
    }
    Ok(components)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn group_selection_includes_descendants_but_not_shared_prefixes() {
        assert!(group_matches("Dator/Programmering/Rust", Some("Dator/Programmering")));
        assert!(group_matches("Dator/Programmering", Some("Dator/Programmering")));
        assert!(!group_matches("Dator/ProgrammeringExtra", Some("Dator/Programmering")));
        assert!(!group_matches("Dator", Some("Dator/Programmering")));
        assert!(group_matches("", Some("")));
        assert!(!group_matches("Dator", Some("")));
    }

    #[test]
    fn library_summary_counts_groups_without_deleted_or_unsupported_entries() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("library.json"),
            serde_json::to_vec(&json!([
                {"hash":"a","title":"A","format":"EPUB","groupName":"Dator/Programmering/Rust"},
                {"hash":"b","title":"B","format":"PDF","groupName":"Dator/Programmering"},
                {"hash":"c","title":"C","format":"EPUB"},
                {"format":"HTML"}, {"deletedAt":1,"format":"PDF"}
            ]))
            .unwrap(),
        )
        .unwrap();
        let source = ReadestLibrary::open(root.path()).unwrap();
        let summary = source.summary();
        assert_eq!(summary.counts, ReadestBookCounts { epub: 2, pdf: 1 });
        assert_eq!(summary.groups.iter().find(|g| g.name == "Dator/Programmering").unwrap().counts.total(), 2);
        assert_eq!(summary.groups.iter().find(|g| g.name.is_empty()).unwrap().counts.total(), 1);
    }
    use crate::reader_transfer::test_importer as importer;

    const FIRST: &[u8] = include_bytes!("../../../book-inspection/tests/fixtures/back-matter-evidence.epub");
    const SECOND: &[u8] = include_bytes!("../../../../tests/fixtures/cloud-cover.epub");

    fn source(root: &Path) {
        let books = [("first", "First book", "Dator/Programmering", FIRST), ("second", "Second book", "Dator/Programmering/Rust", SECOND), ("excluded", "Excluded book", "Dator/ProgrammeringExtra", FIRST)];
        let mut entries = Vec::new();
        for (hash, title, group, bytes) in books {
            let file = root.join(format!("{hash}.epub"));
            fs::write(&file, bytes).unwrap();
            entries.push(json!({"hash":hash,"title":title,"format":"EPUB","groupName":group,"filePath":file}));
        }
        fs::write(root.join("library.json"), serde_json::to_vec(&entries).unwrap()).unwrap();
        fs::create_dir(root.join("first")).unwrap();
        fs::write(
            root.join("first/config.json"),
            serde_json::to_vec(&json!({
                "location":"epubcfi(/6/4)", "progress":[2,10],
                "booknotes":[{"id":"fixture","type":"annotation","cfi":"epubcfi(/6/4!/4/2,/1:0,/1:1)",
                    "text":"A","note":"Fixture note","color":"red","style":"highlight","createdAt":1,"updatedAt":2}]
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn options(mode: ImportMode) -> ReadestImportOptions {
        ReadestImportOptions { parent_id: sync_common::ROOT_DIR_ID, group: Some("Dator/Programmering".into()), mode }
    }

    #[tokio::test]
    async fn selected_group_copies_state_and_reuses_the_unified_subtree_on_retry() {
        let source_root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        source(source_root.path());
        let original_json = fs::read(source_root.path().join("library.json")).unwrap();
        let importer_placement = importer(destination.path()).await;
        let importer_storage = importer_placement.write().await;
        let importer = LibraryImport::new(&importer_storage, std::sync::Arc::new(cpu_host::NativeHost));
        let mut options = options(ImportMode::Copy);
        let parent = importer.storage.database().create_directory(&sync_common::ROOT_DIR_ID, &"Target".into()).await.unwrap().id;
        options.parent_id = parent;
        let report = ReadestLibrary::open(source_root.path()).unwrap().import(&importer, options.clone()).await.unwrap();
        assert_eq!(report.imported, 2);
        assert!(report.failures.is_empty());
        assert_eq!(importer.storage.database().book_count().unwrap(), 2);
        assert!(destination.path().join("Target/Readest/Dator/Programmering/first.epub").is_file());
        assert!(destination.path().join("Target/Readest/Dator/Programmering/Rust/second.epub").is_file());
        assert!(!destination.path().join("Readest").exists());
        let hash = importer.storage.database().book_for_local_content(blake3::hash(FIRST).into()).await.unwrap().unwrap();
        assert_eq!(importer.storage.database().reading_position(&hash).unwrap().as_deref(), Some("epub-location-v2:epubcfi(/6/4)"));
        let annotations = importer.storage.database().annotations(hash).await.unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].note, "Fixture note");
        let repeated = ReadestLibrary::open(source_root.path()).unwrap().import(&importer, options).await.unwrap();
        assert_eq!((repeated.imported, repeated.already_present), (0, 2));
        assert_eq!(importer.storage.database().annotations(hash).unwrap(), annotations);
        assert_eq!(fs::read(source_root.path().join("library.json")).unwrap(), original_json);
        assert_eq!(fs::read(source_root.path().join("first.epub")).unwrap(), FIRST);
        assert_eq!(fs::read_dir(destination.path().join("Target")).unwrap().filter_map(Result::ok).filter(|entry| entry.file_name().to_string_lossy().starts_with("Readest")).count(), 1);
    }

    #[tokio::test]
    async fn a_bad_config_retains_its_source_and_does_not_block_the_next_book() {
        let source_root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        source(source_root.path());
        fs::write(source_root.path().join("first/config.json"), b"invalid JSON").unwrap();
        let importer_placement = importer(destination.path()).await;
        let importer_storage = importer_placement.write().await;
        let importer = LibraryImport::new(&importer_storage, std::sync::Arc::new(cpu_host::NativeHost));
        let report = ReadestLibrary::open(source_root.path()).unwrap().import(&importer, options(ImportMode::Move)).await.unwrap();
        assert_eq!((report.imported, report.failures.len()), (1, 1));
        assert_eq!(fs::read(source_root.path().join("first.epub")).unwrap(), FIRST);
        assert!(!source_root.path().join("second.epub").exists());
    }

    #[tokio::test]
    async fn invalid_group_paths_cannot_escape_the_import_subtree_or_move_a_source() {
        let source_root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        source(source_root.path());
        let path = source_root.path().join("library.json");
        let mut entries: Vec<serde_json::Value> = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        entries.truncate(1);
        entries[0]["groupName"] = json!("../outside");
        fs::write(&path, serde_json::to_vec(&entries).unwrap()).unwrap();
        let importer_placement = importer(destination.path()).await;
        let importer_storage = importer_placement.write().await;
        let importer = LibraryImport::new(&importer_storage, std::sync::Arc::new(cpu_host::NativeHost));
        let options = ReadestImportOptions { group: None, ..options(ImportMode::Move) };
        let report = ReadestLibrary::open(source_root.path()).unwrap().import(&importer, options).await.unwrap();
        assert_eq!(report.failures.len(), 1);
        assert!(source_root.path().join("first.epub").is_file());
        assert!(!destination.path().join("Readest").exists());
        assert_eq!(importer.storage.database().book_count().unwrap(), 0);
    }
}
