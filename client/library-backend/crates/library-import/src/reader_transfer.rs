//! Verified copy/move admission shared by reader-specific metadata adapters.
use crate::{
    LibraryImport,
    foliate::FoliateBook,
    native_move::MovePreparation,
    reader_batch::ReaderImportBatch,
    reader_import::ImportMode,
    readest::{self, ReadestBook, ReadestConfig},
};
use book_inspection::InspectedBook;
use book_model::{AUTHOR_MARC_RELATOR_CODE, BookFormat, BookRecord, BookSubject, BookTocDocument, Contributor, Identifier, LanguageTag, Scheme, Scope};
use client_platform_runtime::executor::run_blocking;
use client_runtime::BackendError;
use content_address::ContentHash;
use filesystem_scanner::ingestion::{IngestedFile, READER_EPUB_METADATA_VERSION, READER_PDF_METADATA_VERSION, ingest_file_with_inspection_caches, ingest_reader_file_with_cached_inspection};
use library_database::transfer::BookTransferState;
use library_replica::{BookAddition, BookFacts, EnrichmentResult};
use serde_json::Value;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use sync_common::{BookId, DirId};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportMetadata {
    Inspect,
    Reader,
}

/// Reader-specific state conversion; admission, copying and moving share one
/// workflow regardless of which reader supplied the metadata.
pub(crate) enum ReaderPreparation {
    Readest { book: ReadestBook, config: ReadestConfig },
    Foliate(FoliateBook),
}

impl ReaderPreparation {
    pub(crate) fn prepare(self, ingested: &IngestedFile, path: &Path, warnings: &mut Vec<String>) -> Result<BookTransferState, BackendError> {
        match self {
            Self::Readest { book, config } => readest::prepare_state(&book, &config, ingested, warnings),
            Self::Foliate(book) => book.prepare_state(ingested, path, warnings),
        }
    }
}

pub(crate) fn reader_inspection_version(format: BookFormat) -> i64 {
    match format {
        BookFormat::Pdf => READER_PDF_METADATA_VERSION,
        BookFormat::Epub => READER_EPUB_METADATA_VERSION,
        _ => book_inspection::inspection_version(format),
    }
}

#[derive(Debug)]
pub struct ReaderBookResult {
    pub content_hash: BookId,
    pub created: bool,
    pub source_removed: bool,
    /// The import committed, but cleanup of private staging failed.
    pub cleanup_error: Option<String>,
    pub skipped_pdf_annotations: usize,
    pub warnings: Vec<String>,
}

pub(crate) async fn import_reader_book(
    importer: &LibraryImport, parent: DirId, source: &Path, format: BookFormat, mode: ImportMode, batch: Option<&mut ReaderImportBatch>, prepare: ReaderPreparation,
) -> Result<ReaderBookResult, BackendError> {
    let name = source.file_name().and_then(|name| name.to_str()).ok_or("invalid reader source filename")?.to_owned();
    if BookFormat::from_file_name(&name) != Some(format) {
        return Err(BackendError::message("reader format does not match the source filename"));
    }
    if mode == ImportMode::Move {
        if fs::symlink_metadata(source)?.file_type().is_symlink() {
            return Err(BackendError::message("copy a reader source symlink instead of moving it"));
        }
        if source.canonicalize()?.starts_with(Path::new(&importer.root).canonicalize()?) {
            return Err(BackendError::message("reader source is already inside the destination library"));
        }
        let (moved, warnings) = importer.move_book_from_path_prepared(parent, source, MovePreparation::Reader(prepare)).await?;
        return Ok(ReaderBookResult { content_hash: moved.content_hash, created: moved.created, source_removed: true, cleanup_error: moved.cleanup_error, skipped_pdf_annotations: 0, warnings });
    }
    if let Some(batch) = batch {
        return import_batched_copy(importer, parent, source, name, batch, prepare).await;
    }
    let ingested = ingest_reader_file(importer, source, name.clone()).await?;
    let source_path = source.to_owned();
    let (state, warnings) = run_blocking(move || {
        let mut warnings = Vec::new();
        let state = prepare.prepare(&ingested, &source_path, &mut warnings)?;
        if library_files::filesystem::fingerprint(&source_path)? != ingested.fingerprint {
            return Err(BackendError::message("reader source changed during preparation"));
        }
        Ok::<_, BackendError>((state, warnings))
    })
    .await
    .map_err(BackendError::operation)??;
    let imported = importer.import_verified_reader_state(parent, name, Box::new(File::open(source)?), state, None).await?;
    Ok(ReaderBookResult { content_hash: imported.content_hash, created: imported.created, source_removed: false, cleanup_error: None, skipped_pdf_annotations: 0, warnings })
}

/// Reader PDFs use external metadata; EPUBs still need their navigation and
/// publication facts. Both routes hash all source bytes and merge fresh notes.
pub(crate) async fn ingest_reader_file(importer: &LibraryImport, source: &Path, name: String) -> Result<IngestedFile, BackendError> {
    ingest_import_file(importer, source, name, ImportMetadata::Reader).await
}

pub(crate) async fn ingest_import_file(importer: &LibraryImport, source: &Path, name: String, metadata: ImportMetadata) -> Result<IngestedFile, BackendError> {
    let format = BookFormat::from_file_name(&name).ok_or("unsupported reader format")?;
    if metadata == ImportMetadata::Reader {
        return Ok(ingest_reader_file_with_cached_inspection(source.to_owned(), name, &*importer.cpu, None, |hash| cached_inspection(importer, source, format, metadata, hash)).await?);
    }
    Ok(ingest_file_with_inspection_caches(
        source.to_owned(),
        name,
        &*importer.cpu,
        |hash| {
            if format == BookFormat::Pdf { importer.database.cached_pdf_reader(hash).map_err(io::Error::other) } else { Ok(None) }
        },
        |hash| cached_inspection(importer, source, format, metadata, hash),
    )
    .await?)
}

fn cached_inspection(importer: &LibraryImport, source: &Path, format: BookFormat, metadata: ImportMetadata, hash: BookId) -> io::Result<Option<InspectedBook>> {
    if metadata == ImportMetadata::Reader && format == BookFormat::Pdf {
        // Basic type validation only. Do not load pages, text, outlines or
        // embedded annotations to reconstruct metadata the reader supplies.
        let mut header = Vec::new();
        File::open(source)?.take(1024).read_to_end(&mut header)?;
        if !header.windows(5).any(|bytes| bytes == b"%PDF-") {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "reader source has no PDF header"));
        }
        return Ok(Some(InspectedBook { metadata: BookRecord { metadata: Default::default(), description: String::new() }, pdf: None, audiobook: None, toc: Vec::new() }));
    }
    let checksum = hash.content_hash().ok_or_else(|| io::Error::other("missing reader checksum"))?;
    let Some(identity) = importer.database.book_for_local_content(checksum).map_err(io::Error::other)? else { return Ok(None) };
    let mut inspection = importer.database.cached_import_inspection(identity, format, book_inspection::inspection_version(format)).map_err(io::Error::other)?;
    if inspection.is_none() && metadata == ImportMetadata::Reader {
        inspection = importer.database.cached_import_inspection(identity, format, reader_inspection_version(format)).map_err(io::Error::other)?;
    }
    if let Some(pdf) = inspection.as_mut().and_then(|inspection| inspection.pdf.as_mut()) {
        if !pdf.valid_for(&identity.to_hex()) {
            return Err(io::Error::other("cached PDF metadata has a different identity"));
        }
        // Ingestion and adapter state describe these byte hashes; the
        // transfer commit retargets them back to the stable library ID.
        pdf.content_hash = hash.to_hex();
    }
    Ok(inspection)
}

async fn ingest_verified_reader_file(importer: &LibraryImport, path: &Path, name: String, digest: (ContentHash, u64, u64)) -> Result<IngestedFile, BackendError> {
    let format = BookFormat::from_file_name(&name).ok_or("unsupported reader format")?;
    Ok(ingest_reader_file_with_cached_inspection(path.to_owned(), name, &*importer.cpu, Some(digest), |hash| cached_inspection(importer, path, format, ImportMetadata::Reader, hash)).await?)
}

/// New local copies are hashed while copying, then inspected from their exact
/// staged bytes. Repeats reuse only previously committed source revisions.
async fn import_batched_copy(importer: &LibraryImport, parent: DirId, source: &Path, name: String, batch: &mut ReaderImportBatch, prepare: ReaderPreparation) -> Result<ReaderBookResult, BackendError> {
    let source = source.canonicalize()?;
    let revision = library_files::filesystem::fingerprint(&source)?;
    let cached = match source.to_str() {
        Some(path) => importer.database.cached_import_source(path, revision)?,
        None => None,
    };
    let cached = match cached {
        Some(checksum) if importer.database.book_for_local_content(checksum)?.is_some() => Some(checksum),
        _ => None,
    };
    let mut import = batch.validate_import(importer, parent, &name)?;
    let (ingested, copied, prepared_path) = if let Some(checksum) = cached {
        let size = fs::metadata(&source)?.len();
        let ingested = ingest_verified_reader_file(importer, &source, name.clone(), (checksum, size, revision)).await?;
        (ingested, None, source.clone())
    } else {
        batch.prepare_directory(importer, parent).await?;
        let relative = batch.native_book_path(importer, parent, &name)?;
        let (actual_name, copied) = batch.assets.copy_import(relative, Box::new(File::open(&source)?), Vec::new(), Default::default()).await?;
        import.file_name = actual_name;
        let digest = copied.copied_digest().ok_or("copied book has no verified checksum")?;
        let path = copied.path().to_owned();
        let ingested = ingest_verified_reader_file(importer, &path, name.clone(), digest).await?;
        (ingested, Some(copied), path)
    };
    let checksum = ingested.content_hash.content_hash().ok_or("reader book has no checksum")?;
    let check_source = source.clone();
    let (state, warnings) = run_blocking(move || {
        let mut warnings = Vec::new();
        let state = prepare.prepare(&ingested, &prepared_path, &mut warnings)?;
        if library_files::filesystem::fingerprint(&prepared_path)? != ingested.fingerprint || library_files::filesystem::fingerprint(&check_source)? != revision {
            return Err(BackendError::message("reader source changed during preparation"));
        }
        Ok::<_, BackendError>((state, warnings))
    })
    .await??;
    let imported = match copied {
        Some(copied) => {
            let staged = copied.verify_transfer_bytes(state.expected_checksum(), None).await?;
            importer.finish_transferred_book(import, staged, state, true, Some(batch)).await?
        }
        None => importer.import_verified_reader_state(parent, name, Box::new(File::open(&source)?), state, Some(batch)).await?,
    };
    if let Some(path) = source.to_str() {
        batch.sources.push((path.to_owned(), revision, checksum));
    }
    Ok(ReaderBookResult { content_hash: imported.content_hash, created: imported.created, source_removed: false, cleanup_error: None, skipped_pdf_annotations: 0, warnings })
}

pub(crate) fn metadata_values(value: &Value) -> Vec<String> {
    match value {
        Value::String(value) => (!value.trim().is_empty()).then(|| value.trim().to_owned()).into_iter().collect(),
        Value::Array(items) => items.iter().flat_map(metadata_values).collect(),
        Value::Object(object) => object.get("name").map(metadata_values).unwrap_or_else(|| object.values().flat_map(metadata_values).take(1).collect()),
        _ => Vec::new(),
    }
}

pub(crate) fn reading_progress(progress: Option<[f64; 2]>, source: &str) -> Result<f32, BackendError> {
    match progress {
        Some([current, total]) if current.is_finite() && total.is_finite() && total > 0.0 && current >= 0.0 && current <= total => Ok((100.0 * current / total) as f32),
        Some(_) => Err(BackendError::message(format!("invalid {source} reading progress"))),
        None => Ok(0.0),
    }
}

pub(crate) struct ReaderMetadata<'a> {
    pub title: &'a str,
    pub author: &'a str,
    pub language: &'a str,
    pub tags: &'a [String],
    pub metadata: &'a Value,
    pub source: &'static str,
}
impl ReaderMetadata<'_> {
    pub fn transfer_state(&self, ingested: &IngestedFile, warnings: &mut Vec<String>) -> Result<BookTransferState, BackendError> {
        let checksum = ingested.content_hash.content_hash().ok_or("reader source has no byte checksum")?;
        let mut inspection = ingested.inspection.clone();
        self.apply_to(&mut inspection.metadata, warnings)?;
        Ok(BookTransferState {
            content_hash: ingested.content_hash,
            byte_checksum: Some(checksum),

            addition: BookAddition {
                facts: BookFacts { added_at: 0, format: ingested.format },
                record: inspection.metadata,
                navigation: (ingested.format != BookFormat::Pdf).then(|| BookTocDocument::entries(inspection.toc)),
                pdf: inspection.pdf,
                recording: None,
                enrichment: Some(EnrichmentResult::NoMatch),
            },
            reading: None,
            chapter_read: None,
            annotations: Vec::new(),
            annotation_source: None,
        })
    }

    pub fn apply_to(&self, record: &mut BookRecord, warnings: &mut Vec<String>) -> Result<(), BackendError> {
        if self.title.trim().is_empty() || self.title.len() > 4096 || self.title.chars().any(char::is_control) {
            return Err(BackendError::message(format!("invalid {} title", self.source)));
        }
        record.title = self.title.to_owned();
        // Readest often stores a complete title while our inspection split it.
        // Never keep an inferred subtitle or repeat words already in that title.
        record.subtitle = metadata_values(&self.metadata["subtitle"]).into_iter().next().filter(|subtitle| !self.title.to_lowercase().contains(&subtitle.to_lowercase()));
        let mut authors = metadata_values(&self.metadata["author"]);
        if authors.is_empty() && !self.author.trim().is_empty() {
            authors.push(self.author.trim().to_owned());
        }
        if !authors.is_empty() {
            record.contributors.retain(|credit| credit.role_code() != AUTHOR_MARC_RELATOR_CODE);
        }
        for name in authors {
            let id = Uuid::new_v5(&Uuid::NAMESPACE_URL, format!("{}/author/{name}", self.source.to_lowercase()).as_bytes());
            record.contributors.push(Contributor::with_id(id, name, AUTHOR_MARC_RELATOR_CODE).map_err(BackendError::operation)?);
        }
        if let Some(description) = metadata_values(&self.metadata["description"]).into_iter().next() {
            record.description = description;
        }
        if !self.metadata["publisher"].is_null() {
            record.book.publishers = metadata_values(&self.metadata["publisher"]);
        }
        if !self.metadata["language"].is_null() {
            let languages = metadata_values(&self.metadata["language"])
                .into_iter()
                .filter_map(|language| match LanguageTag::parse(&language).or_else(|_| LanguageTag::parse(self.language)) {
                    Ok(language) => Some(language),
                    Err(_) => {
                        warnings.push(format!("invalid {} language: {language}", self.source));
                        None
                    }
                })
                .collect::<Vec<_>>();
            if !languages.is_empty() {
                record.book.languages = languages;
            }
        }
        if let Some(date) = metadata_values(&self.metadata["published"]).first() {
            if let Some(year) = date.get(..4).and_then(|year| year.parse::<i32>().ok()).filter(|year| (1..=9999).contains(year)) {
                record.book.year = Some(year);
            }
        }
        for (key, scheme) in [("identifier", Scheme::Unspecified), ("isbn", Scheme::Isbn)] {
            for value in metadata_values(&self.metadata[key]) {
                let identifier = Identifier::new(value, scheme.clone(), Scope::Book).map_err(BackendError::operation)?;
                if !record.book.identifiers.contains(&identifier) {
                    record.book.identifiers.push(identifier);
                }
            }
        }
        if !self.metadata["subject"].is_null() {
            record.book.subjects =
                metadata_values(&self.metadata["subject"]).into_iter().map(|name| BookSubject::new(None, name, &format!("{}:subject", self.source.to_lowercase()), None, None).map_err(BackendError::operation)).collect::<Result<_, _>>()?;
        }
        for tag in self.tags {
            let subject = BookSubject::new(None, tag, &format!("{}:tag", self.source.to_lowercase()), None, None).map_err(BackendError::operation)?;
            if !record.book.subjects.contains(&subject) {
                record.book.subjects.push(subject);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn test_importer(root: &Path) -> LibraryImport {
    use library_database::Database;
    use library_files::AssetStore;
    use library_placement::LibraryPlacement;
    use library_runtime::{events::LibraryEventRouter, scheduler::SyncScheduler};
    use std::sync::Arc;
    let database = Arc::new(Database::open(":memory:").unwrap());
    database.initialize_library().unwrap();
    database.set_asset_storage_policy(false, false).unwrap();
    let assets = AssetStore::open(root.to_str().unwrap()).unwrap();
    let (sync, _) = SyncScheduler::new();
    let (work, _) = SyncScheduler::new();
    let (files, _) = SyncScheduler::new();
    let (enrichment, _) = SyncScheduler::new();
    let (events, _) = LibraryEventRouter::new(sync, work, files, enrichment);
    let placement = Arc::new(LibraryPlacement { database: database.clone(), assets: assets.clone(), events: events.clone() });
    LibraryImport { library_id: uuid::Uuid::new_v4(), database, assets, cpu: Arc::new(client_platform_native::cpu_host::NativeHost), events, placement, root: root.to_str().unwrap().into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::BookDetails;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn reader_progress_checks_bounds_and_nonfinite_values() {
        for source in ["Readest", "Foliate"] {
            assert_eq!(reading_progress(None, source).unwrap(), 0.0);
            for (ratio, expected) in [([0.0, 1.0], 0.0), ([1.0, 4.0], 25.0), ([1.0, 1.0], 100.0)] {
                assert_eq!(reading_progress(Some(ratio), source).unwrap(), expected);
            }
            for ratio in [[-1.0, 1.0], [2.0, 1.0], [0.0, 0.0], [1.0, -1.0], [f64::NAN, 1.0], [1.0, f64::INFINITY]] {
                assert!(reading_progress(Some(ratio), source).is_err());
            }
        }
    }

    #[test]
    fn complete_reader_title_does_not_retain_an_inferred_subtitle() {
        let mut record = BookRecord { metadata: book_model::BookDetails { title: "Main title".into(), subtitle: Some("Subtitle".into()), ..Default::default() }, description: String::new() };
        ReaderMetadata { title: "Main title: Subtitle", author: "", language: "", tags: &[], metadata: &json!({}), source: "Readest" }.apply_to(&mut record, &mut Vec::new()).unwrap();
        assert_eq!(record.title, "Main title: Subtitle");
        assert_eq!(record.subtitle, None);
    }

    struct NoInspection;
    impl cpu_host::CpuHost for NoInspection {
        fn book(&self, _: cpu_host::BookTask, _: cpu_host::BoxedCpuReader) -> client_platform_runtime::executor::BoxedBackendFuture<'_, Result<cpu_host::CpuResponse, BackendError>> {
            panic!("repeat reader import must reuse its current inspection")
        }
        fn request(&self, _: cpu_host::CpuRequest) -> client_platform_runtime::executor::BoxedBackendFuture<'_, Result<cpu_host::CpuResponse, BackendError>> {
            panic!("repeat reader import must not request CPU work")
        }
    }

    struct Unreadable;
    impl std::io::Read for Unreadable {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            panic!("duplicate import must not copy bytes")
        }
    }
    impl std::io::Seek for Unreadable {
        fn seek(&mut self, _: std::io::SeekFrom) -> std::io::Result<u64> {
            panic!("duplicate import must not seek bytes")
        }
    }

    #[tokio::test]
    async fn repeats_reuse_inspection_and_placement_but_general_transfers_verify_bytes() {
        use crate::readest::{ReadestBook, ReadestConfig, import_book_in_batch};
        use sync_common::ROOT_DIR_ID;
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let path = source.path().join("book.epub");
        fs::write(&path, include_bytes!("../../book-inspection/tests/fixtures/back-matter-evidence.epub")).unwrap();
        let book: ReadestBook = serde_json::from_value(json!({"hash":"reader-book","format":"EPUB","title":"Book","filePath":path})).unwrap();
        let mut importer = test_importer(destination.path());
        let first = import_book_in_batch(&importer, source.path(), ROOT_DIR_ID, &book, &ReadestConfig::default(), ImportMode::Copy, None).await.unwrap();
        assert!(importer.database.has_import_inspection(&first.content_hash, BookFormat::Epub, READER_EPUB_METADATA_VERSION).unwrap());
        assert!(!importer.database.has_import_inspection(&first.content_hash, BookFormat::Epub, book_inspection::inspection_version(BookFormat::Epub)).unwrap());
        assert!(importer.database.book_transfer_state(first.content_hash).unwrap().addition.record.book.identifiers.is_empty(), "reader admission must skip the fixture's page-only ISBN");
        let published = destination.path().join("book.epub");
        let revision = library_files::filesystem::fingerprint(&published).unwrap();
        importer.cpu = Arc::new(NoInspection);
        let config = ReadestConfig { booknotes: vec![json!({"id":"new-note","type":"annotation","cfi":"epubcfi(/6/2!/4/2/1:0)","text":"New quote","createdAt":1})], ..Default::default() };
        let repeat = import_book_in_batch(&importer, source.path(), ROOT_DIR_ID, &book, &config, ImportMode::Copy, None).await.unwrap();
        assert!(!repeat.created);
        assert_eq!(importer.database.annotations(first.content_hash).unwrap().len(), 1, "cached inspection must still merge new reader notes");
        let state = importer.database.book_transfer_state(first.content_hash).unwrap();
        let duplicate = importer.import_verified_reader_state(ROOT_DIR_ID, "book.epub".into(), Box::new(Unreadable), state.clone(), None).await.unwrap();
        assert!(!duplicate.created);
        assert_eq!(library_files::filesystem::fingerprint(&published).unwrap(), revision);
        let other_root = tempfile::tempdir().unwrap();
        let other = test_importer(other_root.path());
        let transferred = other.import_transferred_book(ROOT_DIR_ID, "book.epub".into(), Box::new(File::open(&path).unwrap()), state.clone()).await.unwrap();
        assert!(!other.database.has_import_inspection(&transferred.content_hash, BookFormat::Epub, book_inspection::inspection_version(BookFormat::Epub)).unwrap(), "general transfers must not claim a current parser inspection");
        assert!(importer.import_transferred_book(ROOT_DIR_ID, "book.epub".into(), Box::new(std::io::Cursor::new(b"wrong bytes")), state).await.is_err());
        assert_eq!(fs::read_dir(destination.path()).unwrap().filter_map(Result::ok).filter(|entry| entry.file_name().to_string_lossy().contains(".part-")).count(), 0);
    }

    #[tokio::test]
    async fn ordinary_import_rejects_copied_bytes_changed_before_inspection() {
        use library_files::AssetStore;
        use library_model::RelativeBookPath;
        let root = tempfile::tempdir().unwrap();
        let assets = AssetStore::open(root.path().to_str().unwrap()).unwrap();
        let copied = assets.stage_book(RelativeBookPath::parse("book.epub").unwrap(), Box::new(std::io::Cursor::new(b"original"))).await.unwrap();
        fs::write(copied.path(), b"replaced").unwrap();
        let error = filesystem_scanner::ingestion::ingest_copied_file(&copied, "book.epub".into(), &NoInspection).await.err().unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    }

    #[tokio::test]
    async fn reader_pdfs_copy_move_and_repeat_without_inspection_or_background_rescan() {
        use crate::readest::{ReadestBook, ReadestConfig, import_book_in_batch};
        use filesystem_scanner::filesystem_scan::{discover, inspect};
        use sync_common::ROOT_DIR_ID;
        const PDF: &[u8] = include_bytes!("../../../../../shared/pdf-reader-core/tests/fixtures/cover-annotations.pdf");
        for mode in [ImportMode::Copy, ImportMode::Move] {
            let source = tempfile::tempdir().unwrap();
            let destination = tempfile::tempdir().unwrap();
            let path = source.path().join("book.pdf");
            fs::write(&path, PDF).unwrap();
            let book: ReadestBook = serde_json::from_value(json!({"hash":"reader-pdf","format":"PDF","title":"PDF book","author":"Reader author","filePath":path})).unwrap();
            let config: ReadestConfig = serde_json::from_value(json!({"location":"epubcfi(/6/6!/4/2)","progress":[3,10],"booknotes":[{"type":"annotation","cfi":"ignored PDF annotation"}]})).unwrap();
            let mut importer = test_importer(destination.path());
            importer.cpu = Arc::new(NoInspection);
            let first = import_book_in_batch(&importer, source.path(), ROOT_DIR_ID, &book, &config, mode, None).await.unwrap();
            assert!(first.created);
            assert_eq!(first.skipped_pdf_annotations, 1);
            assert_eq!(path.exists(), mode == ImportMode::Copy);
            let state = importer.database.book_transfer_state(first.content_hash).unwrap();
            assert_eq!(state.addition.record.title, "PDF book");
            assert_eq!(state.addition.record.authors().map(|credit| credit.name()).collect::<Vec<_>>(), vec!["Reader author"]);
            assert!(state.addition.pdf.is_none());
            assert!(state.addition.navigation.is_none());
            assert!(state.annotations.is_empty());
            assert_eq!(importer.database.reading_position(&first.content_hash).unwrap().as_deref(), Some("pdfpage(2)"));
            assert!(importer.database.has_import_inspection(&first.content_hash, BookFormat::Pdf, READER_PDF_METADATA_VERSION).unwrap());
            assert!(!importer.database.has_import_inspection(&first.content_hash, BookFormat::Pdf, book_inspection::inspection_version(BookFormat::Pdf)).unwrap());
            fs::write(&path, PDF).unwrap();
            let repeat = import_book_in_batch(&importer, source.path(), ROOT_DIR_ID, &book, &config, mode, None).await.unwrap();
            assert_eq!(repeat.content_hash, first.content_hash);
            assert!(!repeat.created);
            let snapshot = importer.database.scan_snapshot().unwrap();
            let scan = discover(destination.path(), &snapshot);
            let inspected = inspect(destination.path(), &scan, &snapshot, &importer.cpu).await;
            assert!(inspected.readable);
            assert!(inspected.books.is_empty(), "unchanged reader PDFs must not be inspected by a later scan");
        }
    }

    #[tokio::test]
    async fn reader_pdf_without_a_header_fails_and_restores_move_source() {
        use crate::readest::{ReadestBook, ReadestConfig, import_book_in_batch};
        use sync_common::ROOT_DIR_ID;
        for mode in [ImportMode::Copy, ImportMode::Move] {
            let source = tempfile::tempdir().unwrap();
            let destination = tempfile::tempdir().unwrap();
            let path = source.path().join("book.pdf");
            fs::write(&path, b"not a PDF").unwrap();
            let book: ReadestBook = serde_json::from_value(json!({"hash":"bad-pdf","format":"PDF","title":"PDF book","filePath":path})).unwrap();
            let mut importer = test_importer(destination.path());
            importer.cpu = Arc::new(NoInspection);
            assert!(import_book_in_batch(&importer, source.path(), ROOT_DIR_ID, &book, &ReadestConfig::default(), mode, None).await.is_err());
            assert_eq!(fs::read(&path).unwrap(), b"not a PDF");
            assert_eq!(importer.database.book_count().unwrap(), 0);
        }
    }

    #[tokio::test]
    async fn ordinary_pdf_moves_still_request_full_inspection() {
        struct RejectInspection;
        impl cpu_host::CpuHost for RejectInspection {
            fn book(&self, _: cpu_host::BookTask, _: cpu_host::BoxedCpuReader) -> client_platform_runtime::executor::BoxedBackendFuture<'_, Result<cpu_host::CpuResponse, BackendError>> {
                Box::pin(async { Err(BackendError::message("inspection requested")) })
            }
            fn request(&self, _: cpu_host::CpuRequest) -> client_platform_runtime::executor::BoxedBackendFuture<'_, Result<cpu_host::CpuResponse, BackendError>> {
                panic!("unexpected CPU request")
            }
        }
        let source = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let path = source.path().join("book.pdf");
        fs::write(&path, b"%PDF-1.7\n").unwrap();
        let mut importer = test_importer(destination.path());
        importer.cpu = Arc::new(RejectInspection);
        let error = importer.move_book_from_path(sync_common::ROOT_DIR_ID, &path).await.unwrap_err();
        assert!(error.to_string().contains("inspection requested"), "{error}");
        assert!(path.is_file());
        assert_eq!(importer.database.book_count().unwrap(), 0);
    }

    #[test]
    fn absent_external_authors_keep_embedded_authors_for_both_readers() {
        for source in ["Readest", "Foliate"] {
            let mut record =
                BookRecord { metadata: BookDetails { title: "Book".into(), contributors: vec![Contributor::with_id(Uuid::new_v4(), "Embedded author", AUTHOR_MARC_RELATOR_CODE).unwrap()], ..Default::default() }, description: String::new() };
            ReaderMetadata { title: "Book", author: "", language: "", tags: &[], metadata: &json!({}), source }.apply_to(&mut record, &mut Vec::new()).unwrap();
            assert_eq!(record.authors().map(|author| author.name()).collect::<Vec<_>>(), vec!["Embedded author"]);
            ReaderMetadata { title: "Book", author: "", language: "", tags: &[], metadata: &json!({"author":"External author"}), source }.apply_to(&mut record, &mut Vec::new()).unwrap();
            assert_eq!(record.authors().map(|author| author.name()).collect::<Vec<_>>(), vec!["External author"]);
        }
    }
}
