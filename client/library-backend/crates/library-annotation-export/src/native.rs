use crate::failure::{failure, failure_details, input, tracking};
use crate::reader_profiles::{ProfileTarget, ReaderProfiles, readest_hash};
use crate::{foliate, markdown, plan, readest, reconcile};
use book_content::{Book, BookFormat as ContentFormat, BookToc};
use book_inspection::{PdfSourceIdentity, PreparedPdfAnnotationExport, prepare_pdf_annotation_export};
use book_model::Annotation;
use client_platform_native::file_fingerprint::file_fingerprint;
use client_platform_runtime::executor::run_blocking;
use content_address::{ContentHash, hash_file};
use library_database::{Database, DatabaseError};
use library_files::AssetStore;
use library_model::AnnotationExportFailureKind;
use library_model::*;
use md5::{Digest, Md5};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use sync_common::LibraryId;
use tempfile::NamedTempFile;

#[derive(Clone, Default)]
pub struct ExportControl(Arc<AtomicBool>);
impl ExportControl {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Validate a saved destination even when a library has no annotations yet.
pub fn validate_export_folder(root: &Path, folder: &Path) -> io::Result<()> {
    let root = if root.try_exists()? { fs::canonicalize(root)? } else { resolve_destination(root)? };
    outside_library(&folder.join("Annotations.md"), &root).map(|_| ())
}

/// Export locally, importing annotations found in structured destinations before
/// replacement. Files never enter the library or its upload queue.
pub async fn export(database: &Database, assets: &AssetStore, library: LibraryId, options: AnnotationExportOptions, control: ExportControl, mut progress: impl FnMut(AnnotationExportProgress)) -> io::Result<AnnotationExportReport> {
    export_selected_inner(database, assets, library, options, None, control, &mut progress).await
}

/// Keep the complete plan for stable collision numbering, but only rewrite the
/// selected books/formats. Automatic exports never rewrite unchanged PDFs.
pub async fn export_selected(
    database: &Database, assets: &AssetStore, library: LibraryId, options: AnnotationExportOptions, selected: Option<&[AnnotationExportDestination]>, control: ExportControl, mut progress: impl FnMut(AnnotationExportProgress),
) -> io::Result<AnnotationExportReport> {
    export_selected_inner(database, assets, library, options, selected, control, &mut progress).await
}

async fn export_selected_inner(
    database: &Database, assets: &AssetStore, library: LibraryId, options: AnnotationExportOptions, selected: Option<&[AnnotationExportDestination]>, control: ExportControl, progress: &mut dyn FnMut(AnnotationExportProgress),
) -> io::Result<AnnotationExportReport> {
    let mut books = database.annotation_export_snapshot().map_err(io::Error::other)?;
    let mut plan = plan(library, &books, &options)?;
    if let Some(selected) = selected {
        plan.items.retain(|item| selected.iter().any(|selected| selected.book_id == item.book_id && selected.directory_id == item.directory_id && selected.format == item.format));
    }
    let selected_books = plan.items.iter().map(|item| item.book_id).collect::<HashSet<_>>();
    books.retain(|book| selected_books.contains(&book.book_id));
    export_with_profiles(database, assets, library, options, &mut books, plan, control, None, progress).await
}

/// The automatic worker already owns snapshots and destinations for the
/// affected books. Reuse them instead of taking another library-wide snapshot.
pub async fn export_prepared(
    database: &Database, assets: &AssetStore, library: LibraryId, options: AnnotationExportOptions, books: &mut Vec<AnnotationExportBook>, plan: AnnotationExportPlan, control: ExportControl,
    mut progress: impl FnMut(AnnotationExportProgress),
) -> io::Result<AnnotationExportReport> {
    export_with_profiles(database, assets, library, options, books, plan, control, None, &mut progress).await
}

// Keep the export state machine independent of each caller's progress closure.
pub(crate) async fn export_with_profiles(
    database: &Database, assets: &AssetStore, library: LibraryId, options: AnnotationExportOptions, books: &mut Vec<AnnotationExportBook>, mut plan: AnnotationExportPlan, control: ExportControl, supplied_profiles: Option<ReaderProfiles>,
    progress: &mut dyn FnMut(AnnotationExportProgress),
) -> io::Result<AnnotationExportReport> {
    if control.cancelled() {
        return Ok(AnnotationExportReport { cancelled: true, ..Default::default() });
    }
    let mut profiles = HashMap::new();
    let mut preparation_errors = HashMap::new();
    if options.install_readers && plan.items.iter().any(|item| matches!(item.format, AnnotationExportFormat::Foliate | AnnotationExportFormat::Readest)) {
        let catalog = match supplied_profiles {
            Some(profiles) => profiles,
            None => {
                let formats = options.formats.clone();
                run_blocking(move || ReaderProfiles::discover(&formats)).await.map_err(io::Error::other)??
            }
        };
        let mut installed = Vec::new();
        let mut owners = HashMap::new();
        for item in plan.items {
            if control.cancelled() {
                break;
            }
            if !matches!(item.format, AnnotationExportFormat::Foliate | AnnotationExportFormat::Readest) {
                installed.push(item);
                continue;
            }
            let book = books.iter().find(|book| book.book_id == item.book_id).expect("planned book");
            let placement = book.placements.iter().find(|placement| placement.directory_id == item.directory_id).expect("planned placement");
            let result = async {
                let identity = if item.format == AnnotationExportFormat::Foliate {
                    let (_, identifier) = epub_book(assets, book.clone(), placement, item.format).await?;
                    identifier.ok_or_else(|| failure(AnnotationExportFailureKind::Unavailable, "local EPUB is needed to identify its Foliate book data"))?
                } else {
                    let relative = RelativeBookPath::parse(&placement.relative_path).map_err(input)?;
                    let reader =
                        assets.open_book_reader(&book.book_id, vec![relative]).await.map_err(io::Error::other)?.ok_or_else(|| failure(AnnotationExportFailureKind::Unavailable, "local EPUB is needed to identify its Readest book data"))?;
                    run_blocking(move || readest_hash(reader)).await.map_err(io::Error::other)??
                };
                catalog.targets(item.format, &identity)
            }
            .await;
            match result {
                Ok(targets) => {
                    for target in targets {
                        let mut destination = item.clone();
                        destination.path = target.path().to_owned();
                        if owners.insert(destination.path.clone(), destination.book_id).is_some_and(|owner| owner != destination.book_id) {
                            preparation_errors.insert(destination.path.clone(), failure(AnnotationExportFailureKind::Identity, "several library books resolve to the same reader book data; file left unchanged"));
                        }
                        profiles.insert(destination.path.clone(), target);
                        installed.push(destination);
                    }
                }
                Err(error) => {
                    preparation_errors.insert(item.path.clone(), error);
                    installed.push(item);
                }
            }
        }
        plan.items = installed;
    }
    // Markdown exported alongside a PDF must contain its *resulting* local
    // checksum, regardless of the order formats were selected in the prompt.
    plan.items.sort_by_key(|item| if item.format == AnnotationExportFormat::Pdf { 0 } else { 1 });
    let configured_root = assets.native_library_root().to_owned();
    let destinations = plan.items.iter().filter(|item| item.format != AnnotationExportFormat::Pdf && !preparation_errors.contains_key(&item.path)).map(|item| item.path.clone()).collect::<Vec<_>>();
    let root = run_blocking(move || -> io::Result<PathBuf> {
        let root = if configured_root.try_exists()? { fs::canonicalize(configured_root)? } else { resolve_destination(&configured_root)? };
        for destination in destinations {
            outside_library(&destination, &root)?;
        }
        Ok(root)
    })
    .await
    .map_err(io::Error::other)??;
    let exported_at = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_millis().try_into().map_err(io::Error::other)?;
    // Reconcile every structured destination before rendering any output, so
    // Markdown and each selected reader format include all discoveries.
    let mut inputs = HashMap::new();
    if options.conflict == AnnotationExportConflict::Replace || options.install_readers {
        for item in &plan.items {
            if control.cancelled() {
                break;
            }
            if !matches!(item.format, AnnotationExportFormat::Foliate | AnnotationExportFormat::Readest) {
                continue;
            }
            if inputs.contains_key(&item.path) || preparation_errors.contains_key(&item.path) {
                continue;
            }
            let book = books.iter().find(|book| book.book_id == item.book_id).expect("planned book");
            let placement = book.placements.iter().find(|placement| placement.directory_id == item.directory_id).expect("planned placement");
            let result = prepare_json(database, assets, &root, book, placement, item, profiles.get(&item.path), exported_at).await;
            match result {
                Ok(input) => {
                    inputs.insert(item.path.clone(), input);
                }
                Err(error) => {
                    preparation_errors.insert(item.path.clone(), error);
                }
            }
        }
        let selected = plan.items.iter().filter(|item| matches!(item.format, AnnotationExportFormat::Foliate | AnnotationExportFormat::Readest)).map(|item| item.book_id).collect::<HashSet<_>>();
        if !selected.is_empty() {
            let refreshed = database.annotation_export_books(&selected.into_iter().collect::<Vec<_>>()).map_err(io::Error::other)?;
            let mut refreshed = refreshed.into_iter().map(|book| (book.book_id, book)).collect::<HashMap<_, _>>();
            for book in books.iter_mut() {
                if let Some(current) = refreshed.remove(&book.book_id) {
                    *book = current;
                }
            }
        }
    }
    let total = plan.items.len();
    let mut report = AnnotationExportReport { cancelled: control.cancelled(), ..Default::default() };
    for mut item in plan.items {
        if control.cancelled() {
            report.cancelled = true;
            break;
        }
        progress(AnnotationExportProgress { completed: report.results.len(), total, current: Some(item.clone()) });
        // Several placements can resolve to the same reader-owned book data.
        // Publish once, then record the same result for each placement receipt.
        if profiles.contains_key(&item.path) {
            if let Some(previous) = report.results.iter().find(|previous| previous.destination.book_id == item.book_id && previous.destination.format == item.format && previous.destination.path == item.path) {
                let mut result = previous.clone();
                result.destination = item;
                report.results.push(result);
                continue;
            }
        }
        let book = books.iter_mut().find(|book| book.book_id == item.book_id).expect("planned book");
        let placement = book.placements.iter().find(|p| p.directory_id == item.directory_id).expect("planned placement").clone();
        let outcome = if let Some(error) = preparation_errors.get(&item.path) {
            Err(failure(failure_details(error).kind, error))
        } else if item.format == AnnotationExportFormat::Pdf {
            write_pdf(database, assets, &root, book, &placement, &control).await.map(|path| {
                item.path = path;
                AnnotationExportOutcome::Written
            })
        } else {
            let document = match item.format {
                AnnotationExportFormat::Markdown => markdown_document(assets, library, book.clone(), placement.clone(), exported_at).await,
                AnnotationExportFormat::Foliate | AnnotationExportFormat::Readest => epub_document(assets, book.clone(), &placement, item.format, exported_at).await,
                AnnotationExportFormat::Pdf => unreachable!("PDFs are written in place"),
            };
            match document {
                Err(error) => Err(error),
                Ok(mut bytes) => {
                    async {
                        let input = inputs.remove(&item.path);
                        if let Some(Some((_, previous, identities))) = &input {
                            bytes = reconcile::preserve_envelope(previous.clone(), &bytes, identities)?;
                        }
                        if let Some(profile) = profiles.get(&item.path) {
                            bytes = profile.encode(bytes, input.as_ref().and_then(|input| input.as_ref().map(|(_, document, identities)| (document, identities.as_slice()))), exported_at)?;
                        }
                        let expected = input.map(|input| input.map(|(bytes, _, _)| bytes));
                        let path = item.path.clone();
                        let root = root.clone();
                        let conflict = if profiles.contains_key(&item.path) { AnnotationExportConflict::Replace } else { options.conflict };
                        let control = control.clone();
                        run_blocking(move || publish_document(&path, &root, &bytes, conflict, &control, expected.as_ref())).await.map_err(io::Error::other).and_then(|result| result).map(|path| match path {
                            Some(path) => {
                                item.path = path;
                                AnnotationExportOutcome::Written
                            }
                            None => AnnotationExportOutcome::Skipped,
                        })
                    }
                    .await
                }
            }
        };
        let (outcome, failure) = match outcome {
            Ok(outcome) => (outcome, None),
            Err(error) => (AnnotationExportOutcome::Failed(error.to_string()), Some(failure_details(&error))),
        };
        report.results.push(AnnotationExportResult { destination: item, outcome, failure });
        if control.cancelled() {
            report.cancelled = true;
            break;
        }
    }
    progress(AnnotationExportProgress { completed: report.results.len(), total, current: None });
    Ok(report)
}

pub(crate) fn read_destination(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 32 * 1024 * 1024 {
                return Err(failure(AnnotationExportFailureKind::LimitExceeded, "annotation export destination is not a regular file within the 32 MiB import limit"));
            }
            let mut bytes = Vec::new();
            File::open(path)?.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 32 * 1024 * 1024 {
                return Err(failure(AnnotationExportFailureKind::LimitExceeded, "annotation export exceeds the import limit"));
            }
            Ok(Some(bytes))
        }
    }
}

type JsonInput = Option<(Vec<u8>, serde_json::Value, Vec<String>)>;

async fn prepare_json(
    database: &Database, assets: &AssetStore, root: &Path, book: &AnnotationExportBook, placement: &AnnotationExportPlacement, item: &AnnotationExportDestination, profile: Option<&ProfileTarget>, exported_at: u64,
) -> io::Result<JsonInput> {
    let path = outside_library(&item.path, root)?;
    let read_path = path.clone();
    let Some(bytes) = run_blocking(move || read_destination(&read_path)).await.map_err(io::Error::other)?? else { return Ok(None) };
    let mut history = book.clone();
    history.annotations = database.annotation_history(book.book_id).map_err(io::Error::other)?;
    let (history, identifier) = epub_book(assets, history, placement, item.format).await?;
    let expected = if item.format == AnnotationExportFormat::Foliate { foliate(&history, identifier.as_deref())? } else { readest(&history, exported_at)? };
    let mut expected = serde_json::from_slice(&expected).map_err(io::Error::other)?;
    let adapted = profile.map(|profile| profile.decode(&bytes, &mut expected)).transpose()?;
    let (document, mut incoming, identities) = reconcile::read_json(adapted.as_deref().unwrap_or(&bytes), &expected, book.book_id, &history.annotations, item.format)?;
    if !incoming.is_empty() && !book.toc.is_empty() && placement.downloaded && assets.root_available() {
        let relative = RelativeBookPath::parse(&placement.relative_path).map_err(io::Error::other)?;
        if let Some(reader) = assets.open_book_reader(&book.book_id, vec![relative]).await.map_err(io::Error::other)? {
            let mut resolving = book.clone();
            resolving.annotations = incoming.clone();
            incoming = run_blocking(move || -> io::Result<_> {
                let reading = futures_lite::future::block_on(Book::open_reader(ContentFormat::Epub, reader).map_err(input)?.prepare_reading()).map_err(input)?;
                if let Some(bases) = reading.publication_cfi_bases() {
                    crate::foliate::upgrade_legacy(&mut resolving, &bases)?;
                }
                let toc = BookToc::from_reading(&reading, &resolving.toc);
                for (value, resolved) in incoming.iter_mut().zip(&resolving.annotations) {
                    value.toc_ordinal = resolved.anchor.epub_cfi_value().and_then(|cfi| toc.ordinal_for_cfi(cfi));
                }
                Ok(incoming)
            })
            .await
            .map_err(io::Error::other)??;
        }
    }
    if read_destination(&path)?.as_ref() != Some(&bytes) {
        return Err(failure(AnnotationExportFailureKind::Changed, "annotation export file changed during reconciliation; retry"));
    }
    database.import_export_annotations(book.book_id, &incoming).map_err(|error| match error {
        DatabaseError::Message(_) => failure(AnnotationExportFailureKind::InvalidData, error),
        _ => io::Error::other(error),
    })?;
    Ok(Some((bytes, document, identities)))
}

async fn markdown_document(assets: &AssetStore, library: LibraryId, book: AnnotationExportBook, placement: AnnotationExportPlacement, exported_at: u64) -> io::Result<Vec<u8>> {
    if book.format == book_model::BookFormat::Epub && !book.toc.is_empty() && placement.downloaded && assets.root_available() {
        let relative = RelativeBookPath::parse(&placement.relative_path).map_err(io::Error::other)?;
        if let Ok(Some(reader)) = assets.open_book_reader(&book.book_id, vec![relative]).await {
            let mut resolved = book.clone();
            let source_placement = placement.clone();
            if let Ok(Ok(bytes)) = run_blocking(move || {
                let publication = Book::open_reader(ContentFormat::Epub, reader).map_err(input)?;
                let reading = futures_lite::future::block_on(publication.prepare_reading()).map_err(input)?;
                if let Some(bases) = reading.publication_cfi_bases() {
                    crate::foliate::upgrade_legacy(&mut resolved, &bases)?;
                }
                let toc = BookToc::from_reading(&reading, &resolved.toc);
                for annotation in &mut resolved.annotations {
                    annotation.toc_ordinal = annotation.anchor.epub_cfi_value().and_then(|cfi| toc.flattened_ordinal_for_cfi(cfi));
                }
                markdown(library, &resolved, &source_placement, exported_at).map(String::into_bytes)
            })
            .await
            {
                return Ok(bytes);
            }
            // Cached annotations remain exportable when source inspection is
            // unavailable. Resolving headings must not make Markdown depend on it.
        }
    }
    markdown(library, &book, &placement, exported_at).map(String::into_bytes)
}

async fn epub_document(assets: &AssetStore, book: AnnotationExportBook, placement: &AnnotationExportPlacement, format: AnnotationExportFormat, exported_at: u64) -> io::Result<Vec<u8>> {
    let (book, identifier) = epub_book(assets, book, placement, format).await?;
    if format == AnnotationExportFormat::Readest { readest(&book, exported_at) } else { foliate(&book, identifier.as_deref()) }
}

async fn epub_book(assets: &AssetStore, mut book: AnnotationExportBook, placement: &AnnotationExportPlacement, format: AnnotationExportFormat) -> io::Result<(AnnotationExportBook, Option<String>)> {
    let paths = vec![RelativeBookPath::parse(&placement.relative_path).map_err(io::Error::other)?];
    let needs_publication = format == AnnotationExportFormat::Foliate || book.annotations.iter().any(|annotation| !annotation.anchor.is_epub_publication_cfi());
    let reader = if needs_publication && placement.downloaded && assets.root_available() { assets.open_book_reader(&book.book_id, paths.clone()).await.map_err(io::Error::other)? } else { None };
    let had_local_source = reader.is_some();
    let (book, mut identifier) = run_blocking(move || -> io::Result<_> {
        let mut identifier = None;
        if let Some(reader) = reader {
            let publication = Book::open_reader(ContentFormat::Epub, reader).map_err(input)?;
            identifier = publication.publication_identifier();
            if book.annotations.iter().any(|annotation| !annotation.anchor.is_epub_publication_cfi()) {
                let reading = futures_lite::future::block_on(publication.prepare_reading()).map_err(input)?;
                let bases = reading.publication_cfi_bases().ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "EPUB publication addresses are unavailable"))?;
                crate::foliate::upgrade_legacy(&mut book, &bases)?;
            }
        }
        Ok((book, identifier))
    })
    .await
    .map_err(io::Error::other)??;
    if format == AnnotationExportFormat::Readest {
        return Ok((book, identifier));
    }
    if identifier.is_none() && had_local_source {
        // Reopen only for identifier-less EPUBs, avoiding a prefix read for
        // every ordinary export. This hash identifies Foliate data, not books
        // or uploaded canonical content in our library.
        let reader = assets.open_book_reader(&book.book_id, paths).await.map_err(io::Error::other)?.ok_or_else(|| failure(AnnotationExportFailureKind::Unavailable, "EPUB is no longer available for its Foliate identifier"))?;
        identifier = Some(run_blocking(move || foliate_file_identifier(reader)).await.map_err(io::Error::other)??);
    }
    Ok((book, identifier))
}

/// Foliate uses MD5 of the first 10,000,000 bytes (decimal, not 10 MiB).
fn foliate_file_identifier(mut reader: impl Read + Seek) -> io::Result<String> {
    reader.seek(SeekFrom::Start(0))?;
    let mut prefix = reader.take(10_000_000);
    let mut digest = Md5::new();
    let mut buffer = [0; 16 * 1024];
    loop {
        let count = match prefix.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("foliate:{:x}", digest.finalize()))
}

async fn write_pdf(database: &Database, assets: &AssetStore, root: &Path, book: &mut AnnotationExportBook, placement: &AnnotationExportPlacement, control: &ExportControl) -> io::Result<PathBuf> {
    if control.cancelled() {
        return Err(io::Error::other("annotation export cancelled"));
    }
    if !placement.downloaded {
        return Err(failure(AnnotationExportFailureKind::Unavailable, "PDF is not downloaded; only existing local PDFs can be annotated"));
    }
    let relative = RelativeBookPath::parse(&placement.relative_path).map_err(io::Error::other)?;
    let original = root.join(relative.as_str().trim_start_matches('/'));
    let path = fs::canonicalize(&original)?;
    if !path.starts_with(root) || fs::symlink_metadata(&original)?.file_type().is_symlink() {
        return Err(failure(AnnotationExportFailureKind::InvalidData, "PDF source must be a regular file inside this library"));
    }
    let _lease = assets.try_exclusive_lease(&book.book_id).map_err(io::Error::other)?.ok_or_else(|| failure(AnnotationExportFailureKind::Busy, "PDF is being updated; retry the export"))?;
    let source = PdfSourceIdentity { book_id: book.book_id, canonical_hash: book.canonical_hash };
    let known = database.annotation_history(book.book_id).map_err(io::Error::other)?;
    let expected_hash = placement.local_hash.or(book.canonical_hash);
    let read_path = path.clone();
    let (observed_hash, prepared) = run_blocking(move || -> io::Result<_> {
        let observed_hash = checksum(&read_path)?;
        let prepared = prepare_pdf_annotation_export(File::open(&read_path)?, source.book_id, &known).map_err(input)?;
        if expected_hash.is_some_and(|expected| expected != observed_hash) {
            let identity = prepared.source_identity();
            if !identity.is_some_and(|identity| identity.book_id == source.book_id && (source.canonical_hash.is_none() || identity.canonical_hash == source.canonical_hash)) {
                return Err(failure(AnnotationExportFailureKind::InvalidData, "PDF changed since its last library scan; rescan it before exporting annotations"));
            }
        }
        if checksum(&read_path)? != observed_hash {
            return Err(failure(AnnotationExportFailureKind::Changed, "PDF changed during annotation reconciliation; retry"));
        }
        Ok((observed_hash, prepared))
    })
    .await
    .map_err(io::Error::other)??;
    if control.cancelled() {
        return Err(io::Error::other("annotation export cancelled"));
    }
    // Include tombstones while adopting foreign originals, so a stale original
    // cannot survive a library deletion or be imported again on the next run.
    let annotations = database.annotation_history(book.book_id).map_err(io::Error::other)?;
    book.annotations = annotations.iter().filter(|annotation| !annotation.deleted).cloned().collect();
    let stage_path = path.clone();
    let staged = run_blocking(move || stage_pdf(&stage_path, observed_hash, prepared, source, &annotations)).await.map_err(io::Error::other)??;
    if control.cancelled() {
        return Err(io::Error::other("annotation export cancelled"));
    }
    if !assets.root_available() || !database.book_paths(&book.book_id).map_err(io::Error::other)?.contains(&relative) {
        return Err(failure(AnnotationExportFailureKind::Changed, "PDF placement changed during export"));
    }
    let (checksum, size, fingerprint) = run_blocking(move || staged.publish()).await.map_err(io::Error::other)??;
    assets.remember_local_content(book.book_id, checksum).map_err(tracking)?;
    database.record_local_annotation_export(book.book_id, placement.directory_id, checksum, &fingerprint, size).map_err(tracking)?;
    for current in &mut book.placements {
        if current.directory_id == placement.directory_id {
            current.local_hash = Some(checksum);
        }
    }
    Ok(path)
}

struct StagedPdf {
    temporary: NamedTempFile,
    path: PathBuf,
    previous_hash: ContentHash,
    previous_fingerprint: String,
    checksum: ContentHash,
    size: u64,
}
fn stage_pdf(path: &Path, previous_hash: ContentHash, prepared: PreparedPdfAnnotationExport, source: PdfSourceIdentity, annotations: &[Annotation]) -> io::Result<StagedPdf> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.permissions().readonly() {
        return Err(failure(AnnotationExportFailureKind::InvalidData, "PDF is not a writable regular file"));
    }
    let previous_fingerprint = file_fingerprint(path)?.to_string();
    // The document represents previous_hash, checked after parsing. Publication
    // checks that same hash again, including edits made during discovery import.
    let mut temporary = NamedTempFile::new_in(path.parent().ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "PDF has no parent folder"))?)?;
    let checksum = {
        let mut writer = HashingWriter { inner: BufWriter::new(temporary.as_file_mut()), hash: blake3::Hasher::new() };
        prepared.write(&mut writer, source, annotations).map_err(input)?;
        writer.flush()?;
        writer.hash.finalize().into()
    };
    temporary.as_file().set_permissions(metadata.permissions())?;
    temporary.as_file().sync_all()?;
    let size = temporary.as_file().metadata()?.len();
    Ok(StagedPdf { temporary, path: path.to_owned(), previous_hash, previous_fingerprint, checksum, size })
}
impl StagedPdf {
    fn publish(self) -> io::Result<(ContentHash, u64, String)> {
        if file_fingerprint(&self.path)?.to_string() != self.previous_fingerprint || checksum(&self.path)? != self.previous_hash {
            return Err(failure(AnnotationExportFailureKind::Changed, "PDF changed during export; retry against the current file"));
        }
        if self.checksum != self.previous_hash {
            self.temporary.persist(&self.path).map_err(|error| error.error)?;
            sync_folder(self.path.parent().expect("PDF parent")).map_err(tracking)?;
        }
        Ok((self.checksum, self.size, file_fingerprint(&self.path).map_err(tracking)?.to_string()))
    }
}

/// Hash only successfully written bytes, including partial writes. lopdf writes
/// sequentially, so the resulting alias needs no extra pass over the temp file.
struct HashingWriter<W> {
    inner: W,
    hash: blake3::Hasher,
}
impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.hash.update(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn publish_document(path: &Path, root: &Path, bytes: &[u8], conflict: AnnotationExportConflict, control: &ExportControl, expected: Option<&Option<Vec<u8>>>) -> io::Result<Option<PathBuf>> {
    let path = outside_library(path, root)?;
    let parent = path.parent().ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "export file has no parent folder"))?;
    fs::create_dir_all(parent)?;
    let path = outside_library(&path, root)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    if let Some(expected) = expected {
        if read_destination(&path)? != *expected {
            return Err(failure(AnnotationExportFailureKind::Changed, "annotation export file changed during reconciliation; retry"));
        }
    }
    let mut candidate = path.clone();
    let mut index = 2;
    loop {
        if control.cancelled() {
            return Err(io::Error::other("annotation export cancelled"));
        }
        if fs::symlink_metadata(&candidate).is_ok() {
            match conflict {
                AnnotationExportConflict::Skip => return Ok(None),
                AnnotationExportConflict::Replace => {
                    if fs::symlink_metadata(&candidate)?.file_type().is_symlink() || !candidate.is_file() {
                        return Err(failure(AnnotationExportFailureKind::InvalidData, "export destination is not a regular file"));
                    }
                    if let Some(expected) = expected {
                        if read_destination(&candidate)? != *expected {
                            return Err(failure(AnnotationExportFailureKind::Changed, "annotation export file changed during reconciliation; retry"));
                        }
                    }
                    temporary.persist(&candidate).map_err(|error| error.error)?;
                    break;
                }
                AnnotationExportConflict::KeepBoth => {
                    candidate = numbered(&path, index);
                    index += 1;
                    continue;
                }
            }
        }
        match temporary.persist_noclobber(&candidate) {
            Ok(_) => break,
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                if expected.is_some() {
                    return Err(failure(AnnotationExportFailureKind::Changed, "annotation export file appeared during reconciliation; retry"));
                }
                temporary = error.file;
            }
            Err(error) => return Err(error.error),
        }
    }
    sync_folder(parent).map_err(tracking)?;
    Ok(Some(candidate))
}
fn numbered(path: &Path, index: usize) -> PathBuf {
    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("Annotations.md");
    let (stem, extension) = ["foliate.json", "readest.json"].into_iter().find_map(|extension| name.strip_suffix(&format!(".{extension}")).map(|stem| (stem, extension))).unwrap_or_else(|| name.rsplit_once('.').unwrap_or((name, "")));
    path.with_file_name(if extension.is_empty() { format!("{stem} ({index})") } else { format!("{stem} ({index}).{extension}") })
}

/// Resolve existing ancestors, including symlinks, before accepting a new path.
fn outside_library(path: &Path, root: &Path) -> io::Result<PathBuf> {
    let resolved = resolve_destination(path)?;
    if resolved.starts_with(root) {
        return Err(failure(AnnotationExportFailureKind::InvalidData, "new annotation export files must be outside the library"));
    }
    Ok(resolved)
}
fn resolve_destination(path: &Path) -> io::Result<PathBuf> {
    if path.components().any(|part| matches!(part, Component::ParentDir)) {
        return Err(failure(AnnotationExportFailureKind::InvalidData, "export paths cannot contain parent traversal"));
    }
    let path = if path.is_absolute() { path.to_owned() } else { std::env::current_dir()?.join(path) };
    let mut ancestor = path.parent().ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "export file has no parent folder"))?.to_owned();
    let mut suffix = Vec::new();
    while !ancestor.try_exists()? {
        suffix.push(ancestor.file_name().ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "invalid export directory"))?.to_os_string());
        ancestor.pop();
    }
    let mut resolved = fs::canonicalize(ancestor)?;
    for name in suffix.into_iter().rev() {
        resolved.push(name);
    }
    resolved.push(path.file_name().ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "export destination has no filename"))?);
    Ok(resolved)
}
fn sync_folder(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn checksum(path: &Path) -> io::Result<ContentHash> {
    hash_file(&mut File::open(path)?)
}

#[cfg(test)]
mod tests {
    use super::{HashingWriter, checksum, failure_details, foliate_file_identifier, stage_pdf};
    use book_inspection::{PdfSourceIdentity, prepare_pdf_annotation_export};
    use client_platform_native::file_fingerprint::file_fingerprint;
    use library_model::AnnotationExportFailureKind;
    use lopdf::{Document, Object, dictionary};
    use std::{
        fs,
        io::{self, Cursor, Read, Seek, SeekFrom, Write},
    };

    #[test]
    fn output_hash_counts_only_successful_partial_writes() {
        struct PartialWriter {
            bytes: Vec<u8>,
            interrupted: bool,
        }
        impl Write for PartialWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                if self.bytes.len() >= 6 {
                    return Err(io::ErrorKind::WriteZero.into());
                }
                let written = bytes.len().min(2);
                self.bytes.extend_from_slice(&bytes[..written]);
                Ok(written)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut writer = HashingWriter { inner: PartialWriter { bytes: Vec::new(), interrupted: false }, hash: blake3::Hasher::new() };
        writer.write_all(b"abcdef").unwrap();
        assert_eq!(writer.hash.finalize(), blake3::hash(b"abcdef"));
        assert_eq!(writer.write_all(b"rejected").unwrap_err().kind(), io::ErrorKind::WriteZero);
        assert_eq!(writer.inner.bytes, b"abcdef");
        assert_eq!(writer.hash.finalize(), blake3::hash(b"abcdef"), "failed writes must not alter the alias");
    }

    #[test]
    fn prepared_pdf_rejects_source_edits_before_or_after_staging() {
        for change_before_staging in [true, false] {
            let folder = tempfile::tempdir().unwrap();
            let path = folder.path().join("source.pdf");
            let mut document = Document::with_version("1.7");
            let pages = document.new_object_id();
            let page = document.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages});
            document.objects.insert(pages, dictionary! {"Type"=>"Pages", "Kids"=>vec![Object::Reference(page)], "Count"=>1, "MediaBox"=>vec![0.into(), 0.into(), 600.into(), 800.into()]}.into());
            let catalog = document.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages});
            document.trailer.set("Root", catalog);
            document.save(&path).unwrap();
            let original = fs::read(&path).unwrap();
            let previous_hash = checksum(&path).unwrap();
            let source = PdfSourceIdentity { book_id: blake3::hash(b"book").into(), canonical_hash: Some(previous_hash) };
            let prepared = prepare_pdf_annotation_export(Cursor::new(original.clone()), source.book_id, &[]).unwrap();
            let changed = [original.as_slice(), b"\n% external edit"].concat();
            if change_before_staging {
                fs::write(&path, &changed).unwrap();
            }
            let mut staged = stage_pdf(&path, previous_hash, prepared, source, &[]).unwrap();
            assert_eq!(staged.checksum, checksum(staged.temporary.path()).unwrap(), "streamed hash must describe the complete output");
            if !change_before_staging {
                fs::write(&path, &changed).unwrap();
            }
            // Also exercise the content check when the metadata fingerprint matches.
            staged.previous_fingerprint = file_fingerprint(&path).unwrap().to_string();
            let error = staged.publish().unwrap_err();
            let details = failure_details(&error);
            assert_eq!(details.kind, AnnotationExportFailureKind::Changed);
            assert!(!details.file_written);
            assert_eq!(fs::read(&path).unwrap(), changed);
        }
    }

    #[test]
    fn foliate_identifier_hashes_the_decimal_prefix_from_the_start() {
        let mut short = Cursor::new(b"abc");
        short.set_position(2);
        assert_eq!(foliate_file_identifier(short).unwrap(), "foliate:900150983cd24fb0d6963f7d28e17f72");

        let mut bytes = vec![b'a'; 10_000_000];
        bytes.extend_from_slice(b"excluded tail");
        assert_eq!(foliate_file_identifier(Cursor::new(bytes)).unwrap(), "foliate:7095bae098259e0dda4b7acc624de4e2");
    }

    #[test]
    fn foliate_identifier_handles_short_reads_and_interruptions() {
        struct InterruptedReader {
            bytes: Cursor<&'static [u8]>,
            interrupted: bool,
        }
        impl Read for InterruptedReader {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(io::ErrorKind::Interrupted.into());
                }
                self.bytes.read(&mut buffer[..1])
            }
        }
        impl Seek for InterruptedReader {
            fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
                self.bytes.seek(position)
            }
        }
        assert_eq!(foliate_file_identifier(InterruptedReader { bytes: Cursor::new(b"abc"), interrupted: false }).unwrap(), "foliate:900150983cd24fb0d6963f7d28e17f72");
    }
}
