//! Foliate's URI store and per-publication JSON, including EPUB CFIs.
//! Contract: johnfactotum/foliate, gtk4/src/{library,data,annotations}.js.
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

use book_content::{Book, BookFormat as ContainerFormat};
use book_model::{Annotation, BookFormat, EpubCfi, ReadingPosition, PUBLICATION_CFI_PREFIX};
use client_runtime::BackendError;
use filesystem_scanner::ingestion::IngestedFile;
use library_database::transfer::{BookTransferState, ReaderAnnotationSource};
use library_replica::{LibraryStateEntry, ReadingPositionState};
use md5::{Digest, Md5};
use serde::Deserialize;
use serde_json::Value;
use sync_common::DirId;
use url::Url;

use crate::reader_batch::ReaderImportBatch;
use crate::reader_import::{import_books, ImportMode, ReaderBookCounts, ReaderImportBook, ReaderImportOptions, ReaderImportProgress, ReaderImportReport, ReaderImportSource, ReaderLibrarySummary};
use crate::reader_transfer::{import_reader_book, metadata_values, reading_progress, ReaderBookResult, ReaderMetadata, ReaderPreparation};
use crate::LibraryImport;

use reader_formats::{annotation_id, decode_annotation, encoded_identifier, ReaderFormat};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FoliateData {
    #[serde(default)]
    metadata: Value,
    last_location: Option<String>,
    progress: Option<[f64; 2]>,
    #[serde(default)]
    annotations: Vec<Value>,
    #[serde(default, rename = "_bokheim_annotation_ids")]
    annotation_ids: BTreeMap<String, String>,
    #[serde(default)]
    bookmarks: Vec<String>,
}
#[derive(Clone)]
pub(crate) struct FoliateBook {
    identifier: String,
    path: PathBuf,
    data: Result<FoliateData, String>,
    format: BookFormat,
    reference_error: Option<String>,
    title: String,
}

pub struct FoliateLibrary {
    books: Vec<FoliateBook>,
}

impl FoliateLibrary {
    /// Native, Flatpak and Snap use distinct default data roots. Import all
    /// installed variants; byte identity prevents duplicate book placements.
    pub fn default_directories() -> Result<Vec<PathBuf>, BackendError> {
        reader_formats::profiles::foliate_directories().map_err(BackendError::operation)
    }
    pub fn load_default() -> Result<Self, BackendError> {
        let mut books = Vec::new();
        let mut found = false;
        for root in Self::default_directories()? {
            if root.join("library/uri-store.json").is_file() {
                found = true;
                books.extend(Self::open(root)?.books);
            }
        }
        if !found {
            return Err(BackendError::message("could not find Foliate's library in its default locations"));
        }
        Ok(Self { books })
    }
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, BackendError> {
        #[derive(Deserialize)]
        struct UriStore {
            uris: Vec<[String; 2]>,
        }
        let directory = directory.as_ref();
        let store: UriStore = serde_json::from_slice(&fs::read(directory.join("library/uri-store.json"))?).map_err(BackendError::operation)?;
        // Foliate constructs a Map: the most recent duplicate identifier wins.
        let uris: BTreeMap<_, _> = store.uris.into_iter().map(|[id, uri]| (id, uri)).collect();
        let mut books = Vec::new();
        for (identifier, uri) in uris {
            let (path, reference_error) = match local_path(&uri) {
                Ok(path) => (path, None),
                Err(error) => (PathBuf::from(&uri), Some(error.to_string())),
            };
            let Some(format @ (BookFormat::Epub | BookFormat::Pdf)) = path.file_name().and_then(|name| name.to_str()).and_then(BookFormat::from_file_name) else { continue };
            let json_path = directory.join(format!("{}.json", encoded_identifier(&identifier)));
            if !json_path.is_file() {
                continue;
            } // Removed from Foliate's book list.
            let data = fs::read(json_path).map_err(|error| error.to_string()).and_then(|bytes| serde_json::from_slice::<FoliateData>(&bytes).map_err(|error| error.to_string()));
            let title = data.as_ref().ok().and_then(|data| metadata_values(&data.metadata["title"]).into_iter().next()).unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned());
            books.push(FoliateBook { identifier, path, data, format, reference_error, title });
        }
        Ok(Self { books })
    }
    pub fn summary(&self) -> ReaderLibrarySummary {
        let mut counts = ReaderBookCounts::default();
        for book in &self.books {
            match book.format {
                BookFormat::Epub => counts.epub += 1,
                BookFormat::Pdf => counts.pdf += 1,
                _ => {}
            }
        }
        ReaderLibrarySummary { counts, groups: Vec::new(), can_move: true }
    }
    pub async fn import(self, importer: &LibraryImport, options: ReaderImportOptions, cancelled: &dyn Fn() -> bool, progress: impl FnMut(ReaderImportProgress)) -> Result<ReaderImportReport, BackendError> {
        import_books(importer, self.books, options, cancelled, progress).await
    }
}

impl ReaderImportBook for FoliateBook {
    const SOURCE: ReaderImportSource = ReaderImportSource::Foliate;

    fn title(&self) -> &str {
        &self.title
    }

    fn folders(&self) -> Result<Vec<String>, BackendError> {
        Ok(vec!["Foliate".to_owned()])
    }

    fn validate_group(group: Option<&str>) -> Result<(), BackendError> {
        if group.is_some() {
            return Err(BackendError::message("Foliate does not store library groups"));
        }
        Ok(())
    }

    async fn import(
        self, importer: &LibraryImport, resolve_parent: impl FnOnce() -> Result<DirId, BackendError>, mode: ImportMode, batch: Option<&mut ReaderImportBatch>, cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<ReaderBookResult>, BackendError> {
        if let Some(error) = &self.reference_error {
            return Err(BackendError::message(error.clone()));
        }
        let data = self.data.as_ref().map_err(|error| BackendError::message(format!("invalid Foliate book data: {error}")))?;
        // Missing and relocated sources are errors, never guessed by title.
        fs::metadata(&self.path)?;
        if cancelled() {
            return Ok(None);
        }
        let skipped_pdf_annotations = if self.format == BookFormat::Pdf { data.annotations.len() } else { 0 };
        let path = self.path.clone();
        let mut imported = import_reader_book(importer, resolve_parent()?, &path, self.format, mode, batch, ReaderPreparation::Foliate(self)).await?;
        imported.skipped_pdf_annotations = skipped_pdf_annotations;
        Ok(Some(imported))
    }
}

impl FoliateBook {
    pub(crate) fn prepare_state(&self, ingested: &IngestedFile, path: &Path, warnings: &mut Vec<String>) -> Result<BookTransferState, BackendError> {
        let data = self.data.as_ref().map_err(|error| BackendError::message(format!("invalid Foliate book data: {error}")))?;
        verify_identity(&self.identifier, path, ingested)?;
        prepare_state(self, data, ingested, path, warnings)
    }
}

fn local_path(uri: &str) -> Result<PathBuf, BackendError> {
    if let Some(relative) = uri.strip_prefix("~/") {
        return Ok(dirs::home_dir().ok_or("could not locate the home directory")?.join(relative));
    }
    if Path::new(uri).is_absolute() {
        return Ok(uri.into());
    }
    Url::parse(uri).map_err(BackendError::operation)?.to_file_path().map_err(|_| BackendError::message("Foliate book reference is not a local file URI"))
}

// Foliate uses the EPUB package identifier (or PDF dc:identifier); only books
// without one use MD5 of the first 10,000,000 bytes. Verify the inspected file
// before admitting any external state, including the privately claimed move.
fn verify_identity(expected: &str, path: &Path, ingested: &IngestedFile) -> Result<(), BackendError> {
    let matches = if ingested.format == BookFormat::Epub {
        match Book::open_reader(ContainerFormat::Epub, File::open(path)?)?.publication_identifier() {
            Some(actual) => actual == expected,
            None => fallback_identifier(path)? == expected,
        }
    } else if expected.starts_with("foliate:") {
        fallback_identifier(path)? == expected
    } else {
        // Only dictionaries/XMP are needed to verify Foliate's publication
        // reference. Page text and reader metadata remain deferred.
        book_inspection::inspect_pdf_metadata(path).map_err(BackendError::operation)?.metadata.book.identifiers.iter().any(|identifier| identifier.value() == expected)
    };
    if !matches {
        return Err(BackendError::message("Foliate's saved reference points to a different publication; source retained"));
    }
    Ok(())
}
fn fallback_identifier(path: &Path) -> Result<String, BackendError> {
    let mut reader = BufReader::with_capacity(64 * 1024, File::open(path)?.take(10_000_000));
    let mut digest = Md5::new();
    io::copy(&mut reader, &mut digest)?;
    Ok(format!("foliate:{:x}", digest.finalize()))
}

fn prepare_state(book: &FoliateBook, data: &FoliateData, ingested: &IngestedFile, path: &Path, warnings: &mut Vec<String>) -> Result<BookTransferState, BackendError> {
    let mut state = ReaderMetadata { title: &book.title, author: "", language: "", tags: &[], metadata: &data.metadata, source: "Foliate" }.transfer_state(ingested, warnings)?;
    state.reading = reading_state(data, ingested.book_id, book.format)?;
    state.annotations = annotations(data, &book.identifier, ingested.book_id, book.format)?;
    state.annotation_source = Some(ReaderAnnotationSource::Foliate);
    if book.format == BookFormat::Epub && !state.annotations.is_empty() && !ingested.inspection.toc.is_empty() {
        let index = {
            let cfis = state.annotations.iter().filter_map(|annotation| annotation.anchor.epub_cfi_value()).collect::<Vec<_>>();
            File::open(path).and_then(|reader| Book::open_reader(ContainerFormat::Epub, reader)).and_then(|mut book| book.toc_for_cfis(&ingested.inspection.toc, &cfis))
        };
        match index {
            Ok(toc) => {
                for annotation in &mut state.annotations {
                    annotation.toc_ordinal = annotation.anchor.epub_cfi_value().and_then(|cfi| toc.ordinal_for_cfi(cfi));
                }
            }
            Err(error) => warnings.push(format!("Annotations imported without TOC sections: {error}")),
        }
    }
    if !data.bookmarks.is_empty() {
        warnings.push(format!("{} bookmarks remain in Foliate; bookmark import is not supported", data.bookmarks.len()));
    }
    Ok(state)
}

fn reading_state(data: &FoliateData, hash: sync_common::BookId, format: BookFormat) -> Result<Option<LibraryStateEntry>, BackendError> {
    let Some(cfi) = data.last_location.as_deref().filter(|location| !location.is_empty()) else { return Ok(None) };
    EpubCfi::parse(cfi).map_err(BackendError::operation)?;
    let location = if format == BookFormat::Pdf {
        let inner = cfi.strip_prefix("epubcfi(/6/").ok_or("invalid Foliate PDF location")?;
        let step = inner.split(['[', '!', ')']).next().and_then(|value| value.parse::<u32>().ok()).filter(|step| *step >= 2 && step % 2 == 0).ok_or("invalid Foliate PDF page")?;
        ReadingPosition::pdf_page(step / 2 - 1)
    } else {
        ReadingPosition::parse(&format!("{PUBLICATION_CFI_PREFIX}{cfi}")).map_err(BackendError::operation)?
    };
    let progress = reading_progress(data.progress, "Foliate")?;
    Ok(Some(LibraryStateEntry::ReadingPosition { book_id: hash, value: ReadingPositionState { location, progress } }))
}

fn annotations(data: &FoliateData, identifier: &str, hash: sync_common::BookId, format: BookFormat) -> Result<Vec<Annotation>, BackendError> {
    if format != BookFormat::Epub {
        return Ok(Vec::new());
    }
    data.annotations
        .iter()
        .map(|source| {
            let cfi = source["value"].as_str().ok_or("Foliate annotation has no CFI")?;
            let id = source
                .get("_bokheim_id")
                .or_else(|| source.pointer("/_bokheim/model/id"))
                .map(|value| value.as_str().ok_or("invalid Foliate annotation identity"))
                .transpose()?
                .or_else(|| data.annotation_ids.get(cfi).map(String::as_str))
                .map(str::to_owned)
                .unwrap_or_else(|| annotation_id(ReaderFormat::Foliate, identifier, hash, cfi));
            decode_annotation(ReaderFormat::Foliate, source, hash, id).map_err(BackendError::operation)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader_transfer::test_importer;
    use book_model::{AnnotationAnchor, AnnotationStyle};
    use serde_json::json;
    use sync_common::ROOT_DIR_ID;

    const FIRST: &[u8] = include_bytes!("../../book-inspection/tests/fixtures/back-matter-evidence.epub");
    const SECOND: &[u8] = include_bytes!("../../../tests/fixtures/cloud-cover.epub");

    #[test]
    fn fallback_identifier_hashes_only_foliates_ten_million_byte_prefix() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("book.pdf");
        let prefix = vec![b'x'; 10_000_000];
        let mut file = File::create(&path).unwrap();
        file.write_all(&prefix).unwrap();
        file.write_all(b"bytes outside Foliate's identifier prefix").unwrap();
        drop(file);
        assert_eq!(fallback_identifier(&path).unwrap(), format!("foliate:{:x}", Md5::digest(&prefix)));
    }

    fn options(mode: ImportMode) -> ReaderImportOptions {
        ReaderImportOptions { parent_id: ROOT_DIR_ID, group: None, mode }
    }
    fn epub_with_identifier(bytes: &[u8], identifier: &str) -> Vec<u8> {
        use std::io::{Cursor, Write};
        use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};
        let mut source = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut output = ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..source.len() {
            let mut entry = source.by_index(index).unwrap();
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            if name.ends_with(".opf") {
                let mut xml = String::from_utf8(bytes).unwrap().replace("unique-identifier=\"id\"", "unique-identifier=\"foliate-fixture\"");
                if !xml.contains("unique-identifier=") {
                    xml = xml.replace("<package ", "<package unique-identifier=\"foliate-fixture\" ");
                }
                xml = xml.replace("</metadata>", &format!("<dc:identifier id=\"foliate-fixture\">{identifier}</dc:identifier></metadata>"));
                bytes = xml.into_bytes();
            }
            output.start_file(name, SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
            output.write_all(&bytes).unwrap();
        }
        output.finish().unwrap().into_inner()
    }
    fn fixture(root: &Path, entries: &[(&str, &str, &[u8])]) {
        fs::create_dir(root.join("library")).unwrap();
        let mut uris = Vec::new();
        for (identifier, filename, bytes) in entries {
            let path = root.join(filename);
            fs::write(&path, epub_with_identifier(bytes, identifier)).unwrap();
            uris.push([identifier.to_string(), Url::from_file_path(&path).unwrap().to_string()]);
            fs::write(
                root.join(format!("{}.json", encoded_identifier(identifier))),
                serde_json::to_vec(&json!({
                    "metadata":{"identifier":identifier,"title":format!("Title {identifier}"),"author":[{"name":"Author"}]},
                    "lastLocation":"epubcfi(/6/4!/4/2)", "progress":[2,10],
                    "annotations":[{"value":"epubcfi(/6/4!/4/2,/1:0,/1:1)","text":"A","color":"lime","note":"Hello\n",
                        "created":"2024-05-01T14:14:59.202Z","modified":"2024-05-01T14:15:01.202Z"}],
                    "bookmarks":["epubcfi(/6/4!/4/2)"]
                }))
                .unwrap(),
            )
            .unwrap();
        }
        fs::write(root.join("library/uri-store.json"), serde_json::to_vec(&json!({"uris":uris})).unwrap()).unwrap();
    }
    #[test]
    fn exported_root_ids_preserve_annotation_identity_without_extra_properties() {
        let hash = sync_common::BookId::from_bytes([7; 32]);
        let cfi = "epubcfi(/6/2!/4/2,/1:0,/1:7)";
        let data: FoliateData = serde_json::from_value(json!({
            "annotations": [{"value":cfi,"text":"Quote","note":"Note","color":"yellow","created":"","modified":""}],
            "_bokheim_annotation_ids": {cfi: "original-bokheim-id"}
        }))
        .unwrap();
        let imported = annotations(&data, "publication-id", hash, BookFormat::Epub).unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].id, "original-bokheim-id");
        assert_eq!(imported[0].exact_text, "Quote");
    }

    #[test]
    fn uri_decoding_and_identifier_encoding_match_foliate_without_path_escape() {
        assert_eq!(local_path("file:///tmp/B%C3%B6cker/A%20book.epub").unwrap(), PathBuf::from("/tmp/Böcker/A book.epub"));
        assert_eq!(local_path("~/Books/A.epub").unwrap(), dirs::home_dir().unwrap().join("Books/A.epub"));
        assert!(local_path("https://example.org/book.epub").is_err());
        assert!(local_path("file://remote-host/book.epub").is_err());
        assert_eq!(encoded_identifier("urn:isbn:978-1"), "urn%3Aisbn%3A978-1");
        assert_eq!(encoded_identifier("../outside/ö"), "..%2Foutside%2F%C3%B6");
    }
    #[tokio::test]
    async fn copy_preserves_native_cfis_notes_styles_timestamps_and_reuses_destination() {
        let root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fixture(root.path(), &[("urn:isbn:1", "A book ö.epub", FIRST)]);
        let original = fs::read(root.path().join("urn%3Aisbn%3A1.json")).unwrap();
        let importer = test_importer(destination.path());
        let parent = importer.database.create_directory(&ROOT_DIR_ID, &"Target".into()).unwrap().id;
        let options = ReaderImportOptions { parent_id: parent, ..options(ImportMode::Copy) };
        let source = FoliateLibrary::open(root.path()).unwrap();
        assert_eq!(source.summary().counts.total(), 1);
        assert!(source.summary().groups.is_empty());
        let report = source.import(&importer, options.clone(), &|| false, |_| {}).await.unwrap();
        assert_eq!((report.imported, report.completed, report.failures.len()), (1, 1, 0));
        assert_eq!(report.warnings.len(), 1); // Explicitly reports unsupported bookmarks.
        assert!(destination.path().join("Target/Foliate/A book ö.epub").is_file());
        assert!(!destination.path().join("Foliate").exists());
        let hash = importer.database.book_for_local_content(blake3::hash(&epub_with_identifier(FIRST, "urn:isbn:1")).into()).unwrap().unwrap();
        assert_eq!(importer.database.reading_position(&hash).unwrap().as_deref(), Some("epub-location-v2:epubcfi(/6/4!/4/2)"));
        let notes = importer.database.annotations(hash).unwrap();
        assert_eq!(notes.len(), 1);
        let note = &notes[0];
        assert_eq!(note.note, "Hello\n");
        assert_eq!(note.color, 0x00ff00);
        assert_eq!(note.created_at, 1714572899202);
        assert_eq!(note.modified_at, 1714572901202);
        assert!(note.anchor.is_epub_publication_cfi());
        let repeated = FoliateLibrary::open(root.path()).unwrap().import(&importer, options, &|| false, |_| {}).await.unwrap();
        assert_eq!((repeated.imported, repeated.already_present), (0, 1));
        assert_eq!(importer.database.annotations(hash).unwrap(), notes);
        assert_eq!(fs::read(root.path().join("urn%3Aisbn%3A1.json")).unwrap(), original);
        assert_eq!(fs::read(root.path().join("A book ö.epub")).unwrap(), epub_with_identifier(FIRST, "urn:isbn:1"));
    }
    #[tokio::test]
    async fn moved_highlights_reuse_identity_in_copy_and_move_imports_without_restoring_deletions() {
        for mode in [ImportMode::Copy, ImportMode::Move] {
            let root = tempfile::tempdir().unwrap();
            let destination = tempfile::tempdir().unwrap();
            fixture(root.path(), &[("moving", "book.epub", FIRST)]);
            let importer = test_importer(destination.path());
            let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(ImportMode::Copy), &|| false, |_| {}).await.unwrap();
            assert!(report.failures.is_empty(), "{report:?}");
            let book = importer.database.book_for_local_content(blake3::hash(&epub_with_identifier(FIRST, "moving")).into()).unwrap().unwrap();
            let mut original = importer.database.annotations(book).unwrap().pop().unwrap();
            original.note = "Edited locally".into();
            original.style = AnnotationStyle::Underline;
            original.anchor = original.anchor.with_recurring_scope(Some(book_model::RecurringScope::Book));
            original.modified_at += 1;
            importer.database.upsert_annotation(&original).unwrap();
            let path = root.path().join("moving.json");
            let mut data: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            data["annotations"][0]["value"] = json!("epubcfi(/6/4!/4/4,/1:0,/1:1)");
            data["annotations"][0]["text"] = json!("B");
            data["annotations"][0]["modified"] = json!("2024-05-02T14:15:01.202Z");
            if matches!(mode, ImportMode::Move) {
                // A retained ID is stronger evidence than an absent date.
                data["annotations"][0]["_bokheim"] = json!({"model":{"id":original.id}});
                data["annotations"][0]["created"] = json!("");
            }
            fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
            let watermark = importer.database.sync_outbox_watermark().unwrap();
            let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(mode), &|| false, |_| {}).await.unwrap();
            assert!(report.failures.is_empty(), "{report:?}");
            let notes = importer.database.annotations(book).unwrap();
            assert_eq!(notes.len(), 1);
            assert_eq!(notes[0].id, original.id);
            assert_eq!(notes[0].note, original.note);
            assert_eq!(notes[0].style, original.style);
            assert_eq!(notes[0].created_at, original.created_at);
            assert_eq!(notes[0].anchor.recurring_scope(), original.anchor.recurring_scope());
            assert_eq!(notes[0].anchor.epub_cfi_value(), data["annotations"][0]["value"].as_str());
            assert_eq!(notes[0].exact_text, "B");
            assert!(importer.database.sync_outbox_watermark().unwrap() > watermark);
            // Move consumes the source. Recreate it to test a subsequent import.
            fs::write(root.path().join("book.epub"), epub_with_identifier(FIRST, "moving")).unwrap();
            importer.database.delete_annotation(&original.id, 1_800_000_000_000).unwrap();
            data["annotations"][0]["value"] = json!("epubcfi(/6/4!/4/6,/1:0,/1:1)");
            data["annotations"][0]["modified"] = json!("2027-05-02T14:15:01.202Z");
            fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
            let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(mode), &|| false, |_| {}).await.unwrap();
            assert!(report.failures.is_empty(), "{report:?}");
            assert!(importer.database.annotations(book).unwrap().is_empty());
            assert_eq!(importer.database.annotation_history(book).unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn ambiguous_moved_highlights_roll_back_foliate_move_import() {
        let root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fixture(root.path(), &[("ambiguous", "book.epub", FIRST)]);
        let importer = test_importer(destination.path());
        let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(ImportMode::Copy), &|| false, |_| {}).await.unwrap();
        assert!(report.failures.is_empty());
        let book = importer.database.book_for_local_content(blake3::hash(&epub_with_identifier(FIRST, "ambiguous")).into()).unwrap().unwrap();
        let mut second = importer.database.annotations(book).unwrap().pop().unwrap();
        second.id = "same-creation-time".into();
        second.anchor = AnnotationAnchor::epub_publication_cfi("epubcfi(/6/4!/4/4,/1:0,/1:1)");
        importer.database.upsert_annotation(&second).unwrap();
        let notes = importer.database.annotations(book).unwrap();
        let watermark = importer.database.sync_outbox_watermark().unwrap();
        let path = root.path().join("ambiguous.json");
        let mut data: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        data["annotations"][0]["value"] = json!("epubcfi(/6/4!/4/6,/1:0,/1:1)");
        let bytes = serde_json::to_vec(&data).unwrap();
        fs::write(&path, &bytes).unwrap();
        let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(ImportMode::Move), &|| false, |_| {}).await.unwrap();
        assert_eq!(report.failures.len(), 1, "{report:?}");
        assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), epub_with_identifier(FIRST, "ambiguous"));
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(importer.database.annotations(book).unwrap(), notes);
        assert_eq!(importer.database.sync_outbox_watermark().unwrap(), watermark);
    }

    #[tokio::test]
    async fn imported_cfis_match_nested_toc_sections_and_export_under_their_headings() {
        use library_annotation_export::{export, ExportControl};
        use library_model::{AnnotationExportConflict, AnnotationExportFormat, AnnotationExportOptions, AnnotationExportOutcome};
        let root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let epub = include_bytes!("../../../../book-content/tests/fixtures/annotation-toc.epub");
        fixture(root.path(), &[("toc-book", "book.epub", epub)]);
        let data_path = root.path().join("toc-book.json");
        let mut data: Value = serde_json::from_slice(&fs::read(&data_path).unwrap()).unwrap();
        data["annotations"] = json!([
            {"value":"epubcfi(/8[sp!ine]/4[one^]id]!/4/4,/1:0,/1:6)","text":"Before quote","note":"First note"},
            {"value":"epubcfi(/8/4!/4/6/4,/1:0,/1:5)","text":"After quote","note":"Section note"},
            {"value":"epubcfi(/8/8!/4/2,/1:0,/1:4)","text":"Next quote","note":"Next note"},
            {"value":"epubcfi(/8/6!/4/2,/1:0,/1:4)","text":"Unresolved quote","note":"Keep unresolved"}
        ]);
        fs::write(&data_path, serde_json::to_vec(&data).unwrap()).unwrap();
        let importer = test_importer(destination.path());
        let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(ImportMode::Copy), &|| false, |_| {}).await.unwrap();
        assert_eq!((report.imported, report.failures.len()), (1, 0), "{report:?}");
        let book = importer.database.book_for_local_content(blake3::hash(&epub_with_identifier(epub, "toc-book")).into()).unwrap().unwrap();
        let notes = importer.database.annotations(book).unwrap();
        for (text, ordinal) in [("Before quote", Some(0)), ("After quote", Some(1)), ("Next quote", Some(2)), ("Unresolved quote", None)] {
            assert_eq!(notes.iter().find(|note| note.exact_text == text).unwrap().toc_ordinal, ordinal, "{text}");
        }
        let export_options = AnnotationExportOptions {
            destination: output.path().to_owned(),
            formats: vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Foliate],
            install_readers: false,
            conflict: AnnotationExportConflict::Replace,
            overrides: vec![],
        };
        let report = export(&importer.database, &importer.assets, ROOT_DIR_ID, export_options.clone(), ExportControl::default(), |_| {}).await.unwrap();
        assert_eq!(report.results.len(), 2);
        assert!(report.results.iter().all(|result| matches!(result.outcome, AnnotationExportOutcome::Written)), "{report:?}");
        let markdown_path = &report.results.iter().find(|result| result.destination.format == AnnotationExportFormat::Markdown).unwrap().destination.path;
        let markdown = fs::read_to_string(markdown_path).unwrap();
        for (heading, quote) in [("### Chapter One", "Before quote"), ("#### Second section", "After quote"), ("### Chapter Two", "Next quote"), ("## Other annotations", "Unresolved quote")] {
            let heading = markdown.find(&format!("{heading}\n")).unwrap();
            assert!(heading < markdown.find(quote).unwrap(), "{markdown}");
        }
        assert!(markdown.contains("## Part\n"), "unresolved grouping headings remain in the export");

        // Import-before-export uses the same matcher for new Foliate JSON notes.
        let foliate_path = &report.results.iter().find(|result| result.destination.format == AnnotationExportFormat::Foliate).unwrap().destination.path;
        let mut external: Value = serde_json::from_slice(&fs::read(foliate_path).unwrap()).unwrap();
        external["annotations"].as_array_mut().unwrap().push(json!({"value":"epubcfi(/8/4!/4/6/2,/1:0,/1:6)","text":"New external quote","note":"New external note","color":"yellow"}));
        fs::write(foliate_path, serde_json::to_vec(&external).unwrap()).unwrap();
        let refreshed = export(&importer.database, &importer.assets, ROOT_DIR_ID, export_options, ExportControl::default(), |_| {}).await.unwrap();
        assert!(refreshed.results.iter().all(|result| matches!(result.outcome, AnnotationExportOutcome::Written)), "{refreshed:?}");
        let imported = importer.database.annotations(book).unwrap().into_iter().find(|note| note.exact_text == "New external quote").unwrap();
        assert_eq!(imported.toc_ordinal, Some(1));
        assert!(fs::read_to_string(markdown_path).unwrap().contains("New external quote"));

        // Re-import fills missing chapter metadata without replacing local edits.
        let mut older = notes.iter().find(|note| note.exact_text == "Before quote").unwrap().clone();
        older.toc_ordinal = None;
        older.note = "Edited in Bokheim".into();
        older.modified_at = 10;
        importer.database.upsert_annotation(&older).unwrap();
        let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(ImportMode::Copy), &|| false, |_| {}).await.unwrap();
        assert!(report.failures.is_empty(), "{report:?}");
        let current = importer.database.annotations(book).unwrap().into_iter().find(|note| note.id == older.id).unwrap();
        assert_eq!(current.toc_ordinal, Some(0));
        assert_eq!(current.note, older.note);
        assert_eq!(current.modified_at, older.modified_at);
        importer.database.delete_annotation(&older.id, 11).unwrap();
        let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(ImportMode::Copy), &|| false, |_| {}).await.unwrap();
        assert!(report.failures.is_empty(), "{report:?}");
        assert!(!importer.database.annotations(book).unwrap().iter().any(|note| note.id == older.id));
    }

    #[tokio::test]
    async fn cancellation_commits_copied_books_together_and_leaves_remaining_sources() {
        use std::cell::Cell;
        let root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fixture(root.path(), &[("a", "a.epub", FIRST), ("b", "b.epub", SECOND)]);
        let importer = test_importer(destination.path());
        let cancelled = Cell::new(false);
        let mut observed = Vec::new();
        let report = FoliateLibrary::open(root.path())
            .unwrap()
            .import(&importer, options(ImportMode::Copy), &|| cancelled.get(), |progress| {
                assert_eq!(importer.database.book_count().unwrap(), 0, "copied book metadata waits for batch completion");
                observed.push((progress.completed, progress.current.is_some()));
                if progress.completed == 1 {
                    cancelled.set(true);
                }
            })
            .await
            .unwrap();
        assert!(report.cancelled);
        assert_eq!((report.completed, report.total, report.imported), (1, 2, 1));
        assert_eq!(observed, [(0, true), (1, false)]);
        assert_eq!(importer.database.book_count().unwrap(), 1);
        assert!(root.path().join("a.epub").is_file());
        assert!(root.path().join("b.epub").is_file());
        assert!(destination.path().join("Foliate/a.epub").is_file());
        assert!(!destination.path().join("Foliate/b.epub").exists());
    }

    #[tokio::test]
    async fn cancelling_on_start_keeps_the_current_source_in_both_adapters() {
        use crate::reader_import::ReaderLibrary;
        use crate::readest::ReadestLibrary;
        use std::cell::Cell;
        let root = tempfile::tempdir().unwrap();
        fixture(root.path(), &[("a", "a.epub", FIRST)]);
        fs::write(
            root.path().join("library.json"),
            serde_json::to_vec(&json!([
                {"hash":"a","format":"EPUB","title":"A","filePath":root.path().join("a.epub")}
            ]))
            .unwrap(),
        )
        .unwrap();
        let original = fs::read(root.path().join("a.epub")).unwrap();
        let libraries = [ReaderLibrary::Foliate(FoliateLibrary::open(root.path()).unwrap()), ReaderLibrary::Readest(ReadestLibrary::open(root.path()).unwrap())];
        for library in libraries {
            let destination = tempfile::tempdir().unwrap();
            let importer = test_importer(destination.path());
            let cancelled = Cell::new(false);
            let report = library
                .import(&importer, options(ImportMode::Move), &|| cancelled.get(), |progress| {
                    assert!(progress.current.is_some());
                    cancelled.set(true);
                })
                .await
                .unwrap();
            assert!(report.cancelled);
            assert_eq!((report.completed, report.total, report.imported), (0, 1, 0));
            assert!(report.failures.is_empty());
            assert_eq!(importer.database.book_count().unwrap(), 0);
            assert_eq!(fs::read(root.path().join("a.epub")).unwrap(), original);
        }
    }

    #[tokio::test]
    async fn invalid_annotation_rolls_back_move_and_does_not_block_other_books() {
        let root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fixture(root.path(), &[("a", "a.epub", FIRST), ("b", "b.epub", SECOND)]);
        let path = root.path().join("a.json");
        let mut data: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        data["annotations"][0]["value"] = json!("invalid CFI");
        fs::write(path, serde_json::to_vec(&data).unwrap()).unwrap();
        let importer = test_importer(destination.path());
        let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(ImportMode::Move), &|| false, |_| {}).await.unwrap();
        assert_eq!((report.imported, report.failures.len()), (1, 1));
        assert_eq!(fs::read(root.path().join("a.epub")).unwrap(), epub_with_identifier(FIRST, "a"));
        assert!(!root.path().join("b.epub").exists());
        assert_eq!(fs::read(destination.path().join("Foliate/b.epub")).unwrap(), epub_with_identifier(SECOND, "b"));
    }
    #[tokio::test]
    async fn cancelling_move_finishes_current_book_and_keeps_remaining_sources() {
        use std::cell::Cell;
        let root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fixture(root.path(), &[("a", "a.epub", FIRST), ("b", "b.epub", SECOND)]);
        let importer = test_importer(destination.path());
        let cancel = Cell::new(false);
        let report = FoliateLibrary::open(root.path())
            .unwrap()
            .import(&importer, options(ImportMode::Move), &|| cancel.get(), |progress| {
                if progress.completed == 1 {
                    cancel.set(true)
                }
            })
            .await
            .unwrap();
        assert!(report.cancelled);
        assert_eq!((report.imported, report.completed), (1, 1));
        assert!(!root.path().join("a.epub").exists());
        assert_eq!(fs::read(root.path().join("b.epub")).unwrap(), epub_with_identifier(SECOND, "b"));
    }
    #[tokio::test]
    async fn missing_and_remote_sources_fail_individually_and_keep_other_books_importable() {
        let root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fixture(root.path(), &[("a", "a.epub", FIRST), ("b", "b.epub", FIRST), ("c", "c.epub", SECOND)]);
        fs::remove_file(root.path().join("a.epub")).unwrap();
        let store = root.path().join("library/uri-store.json");
        let mut data: Value = serde_json::from_slice(&fs::read(&store).unwrap()).unwrap();
        data["uris"][1][1] = json!("https://example.org/b.epub");
        fs::write(store, serde_json::to_vec(&data).unwrap()).unwrap();
        let importer = test_importer(destination.path());
        let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(ImportMode::Move), &|| false, |_| {}).await.unwrap();
        assert_eq!((report.total, report.imported, report.failures.len()), (3, 1, 2));
        assert_eq!(fs::read(root.path().join("b.epub")).unwrap(), epub_with_identifier(FIRST, "b"));
    }
    #[tokio::test]
    async fn stale_reference_keeps_the_replacement_book_and_rejects_external_state() {
        for mode in [ImportMode::Copy, ImportMode::Move] {
            let root = tempfile::tempdir().unwrap();
            let destination = tempfile::tempdir().unwrap();
            fixture(root.path(), &[("expected", "book.epub", FIRST)]);
            // The same saved pathname now contains another, valid publication.
            fs::write(root.path().join("book.epub"), SECOND).unwrap();
            let importer = test_importer(destination.path());
            let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(mode), &|| false, |_| {}).await.unwrap();
            assert_eq!((report.imported, report.failures.len()), (0, 1));
            assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), SECOND);
            assert_eq!(importer.database.book_count().unwrap(), 0);
            assert!(!destination.path().join("Foliate/book.epub").exists());
        }
    }

    #[tokio::test]
    async fn generated_identifier_and_sparse_metadata_preserve_the_embedded_author() {
        let root = tempfile::tempdir().unwrap();
        let destination = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("library")).unwrap();
        let path = root.path().join("book.epub");
        fs::write(&path, FIRST).unwrap();
        let identifier = fallback_identifier(&path).unwrap();
        fs::write(root.path().join("library/uri-store.json"), serde_json::to_vec(&json!({"uris":[[identifier,Url::from_file_path(&path).unwrap().to_string()]]})).unwrap()).unwrap();
        fs::write(root.path().join(format!("{}.json", encoded_identifier(&identifier))), serde_json::to_vec(&json!({"metadata":{"title":"Sparse metadata"}})).unwrap()).unwrap();
        let importer = test_importer(destination.path());
        let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(ImportMode::Copy), &|| false, |_| {}).await.unwrap();
        assert_eq!((report.imported, report.failures.len()), (1, 0));
        let hash = importer.database.book_for_local_content(blake3::hash(FIRST).into()).unwrap().unwrap();
        let record = importer.database.book_transfer_state(hash).unwrap().addition.record;
        assert_eq!(record.authors().map(|author| author.name()).collect::<Vec<_>>(), vec!["Ada Author"]);
    }

    #[test]
    fn generated_identifiers_hash_exactly_the_first_ten_million_bytes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("book.pdf");
        let mut bytes = vec![b'a'; 10_000_000];
        bytes.extend_from_slice(b"tail");
        fs::write(&path, &bytes).unwrap();
        let expected = format!("foliate:{:x}", Md5::digest(&bytes[..10_000_000]));
        assert_eq!(fallback_identifier(&path).unwrap(), expected);
        bytes[10_000_000] = b'b';
        fs::write(&path, &bytes).unwrap();
        assert_eq!(fallback_identifier(&path).unwrap(), expected);
    }

    #[tokio::test]
    async fn pdf_library_imports_verify_declared_or_generated_identity_without_page_inspection() {
        use std::io::Write;
        let xml = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier>external-pdf-id</dc:identifier></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let mut pdf = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for object in [
            "<< /Type /Catalog /Pages 3 0 R /Metadata 2 0 R >>".to_owned(),
            format!("<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n{xml}\nendstream", xml.len()),
            "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 3 0 R /MediaBox [0 0 600 800] >>".to_owned(),
        ] {
            offsets.push(pdf.len());
            write!(pdf, "{} 0 obj\n{object}\nendobj\n", offsets.len()).unwrap();
        }
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in offsets {
            writeln!(pdf, "{offset:010} 00000 n ").unwrap();
        }
        write!(pdf, "trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").unwrap();
        for mode in [ImportMode::Copy, ImportMode::Move] {
            for declared in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let destination = tempfile::tempdir().unwrap();
                fs::create_dir(root.path().join("library")).unwrap();
                let path = root.path().join("book.pdf");
                fs::write(&path, &pdf).unwrap();
                let identifier = if declared { "external-pdf-id".to_owned() } else { fallback_identifier(&path).unwrap() };
                fs::write(root.path().join("library/uri-store.json"), serde_json::to_vec(&json!({"uris":[[identifier,Url::from_file_path(&path).unwrap().to_string()]]})).unwrap()).unwrap();
                fs::write(
                    root.path().join(format!("{}.json", encoded_identifier(&identifier))),
                    serde_json::to_vec(&json!({"metadata":{"title":"Foliate PDF","author":"External author"},"lastLocation":"epubcfi(/6/2)","annotations":[{"ignored":"PDF annotation"}]})).unwrap(),
                )
                .unwrap();
                let importer = test_importer(destination.path());
                let report = FoliateLibrary::open(root.path()).unwrap().import(&importer, options(mode), &|| false, |_| {}).await.unwrap();
                assert!(report.failures.is_empty(), "{report:?}");
                assert_eq!(report.imported, 1);
                assert_eq!(path.exists(), mode == ImportMode::Copy);
                let hash = importer.database.book_for_local_content(blake3::hash(&pdf).into()).unwrap().unwrap();
                let state = importer.database.book_transfer_state(hash).unwrap();
                assert_eq!(state.addition.record.title, "Foliate PDF");
                assert!(state.addition.pdf.is_none());
                assert!(state.addition.navigation.is_none());
                assert!(state.annotations.is_empty());
                assert_eq!(importer.database.reading_position(&hash).unwrap().as_deref(), Some("pdfpage(0)"));
            }
        }
    }

    #[test]
    fn pdf_positions_convert_to_pages_and_pdf_notes_are_never_interpreted() {
        let data: FoliateData = serde_json::from_value(json!({
            "lastLocation":"epubcfi(/6/56!/4/2)", "progress":[28,444],
            "annotations":[{"unrecognized":"PDF text layer annotation"}]
        }))
        .unwrap();
        let hash = blake3::hash(FIRST).into();
        let LibraryStateEntry::ReadingPosition { value, .. } = reading_state(&data, hash, BookFormat::Pdf).unwrap().unwrap() else { panic!("expected reading position") };
        assert_eq!(value.location, ReadingPosition::pdf_page(27));
        assert!((value.progress - 100.0 * 28.0 / 444.0).abs() < 0.001);
        assert!(annotations(&data, "pdf", hash, BookFormat::Pdf).unwrap().is_empty());
        assert!(annotations(&data, "epub", hash, BookFormat::Epub).is_err());
    }

    #[test]
    fn annotation_styles_colors_and_old_missing_dates_are_preserved() {
        for (value, expected) in [("yellow", 0xffff00), ("orange", 0xffa500), ("#1a2", 0x11aa22), ("#123abc", 0x123abc), ("rgb(1, 2, 3)", 0x010203), ("rgba(1,2,3,1)", 0x010203)] {
            assert_eq!(book_model::parse_annotation_color(value).unwrap(), Some(expected));
        }
        assert_eq!(book_model::parse_annotation_color("#1234567").unwrap(), None);
        assert!(book_model::parse_annotation_color("rgb(999,0,0)").is_err());
        let hash = blake3::hash(FIRST).into();
        for (color, style) in [("underline", AnnotationStyle::Underline), ("squiggly", AnnotationStyle::Squiggly), ("strikethrough", AnnotationStyle::Strikethrough)] {
            let note = json!({"value":"epubcfi(/6/4)","color":color,"note":"Old note"});
            let converted = decode_annotation(ReaderFormat::Foliate, &note, hash, annotation_id(ReaderFormat::Foliate, "id", hash, "epubcfi(/6/4)")).unwrap();
            assert_eq!(converted.style, style);
            assert_eq!(converted.created_at, 0);
            assert_eq!(converted.modified_at, 0);
            assert_eq!(converted.id, decode_annotation(ReaderFormat::Foliate, &note, hash, annotation_id(ReaderFormat::Foliate, "id", hash, "epubcfi(/6/4)")).unwrap().id);
        }
    }
}
