//! Ingest a native book already present on disk, whether copied or discovered.

use std::io::{self, Seek};
use std::path::{Path, PathBuf};

use book_inspection::{CachedPdfReader, InspectedBook};
use book_model::BookFormat;
use client_platform_runtime::executor;
use content_address::{ContentHash, hash_reader_with_size};
use library_files::asset_store::CopiedBook;
use sync_common::BookId;

/// Verified PDFs admitted with metadata from another reader. This is not a
/// completed PDF parser inspection; reader data is loaded when the book opens.
/// Keep it distinct from every positive parser version.
pub const READER_PDF_METADATA_VERSION: i64 = -1;
/// EPUB package/navigation admission, without copyright-page evidence scanning.
pub const READER_EPUB_METADATA_VERSION: i64 = -2;

pub struct IngestedFile {
    pub source_identity: Option<book_inspection::PdfSourceIdentity>,
    pub book_id: BookId,
    pub size_bytes: u64,
    pub fingerprint: u64,
    pub format: BookFormat,
    pub inspection: book_inspection::InspectedBook,
}

/// Hash and inspect the same file bytes, rejecting concurrent changes.
pub async fn ingest_file(path: PathBuf, name: String, host: &dyn cpu_host::CpuHost) -> io::Result<IngestedFile> {
    ingest_file_with_cached_inspection(path, name, host, async |_| Ok(None)).await
}

/// Reinspect bibliographic evidence while reusing byte-matched reader facts.
pub async fn ingest_file_with_pdf_reader(path: PathBuf, name: String, host: &dyn cpu_host::CpuHost, cached_pdf: Option<CachedPdfReader>) -> io::Result<IngestedFile> {
    ingest_file_inner(path, name, host, None, false, async move |_| Ok(cached_pdf), async |_| Ok(None)).await
}

pub async fn ingest_copied_file_with_pdf_reader(copied: &CopiedBook, name: String, host: &dyn cpu_host::CpuHost, cached_pdf: Option<CachedPdfReader>) -> io::Result<IngestedFile> {
    ingest_file_inner(copied.path().to_owned(), name, host, copied.copied_digest(), false, async move |_| Ok(cached_pdf), async |_| Ok(None)).await
}

/// Inspect unchanged staged bytes using the checksum computed during copying.
pub async fn ingest_copied_file(copied: &CopiedBook, name: String, host: &dyn cpu_host::CpuHost) -> io::Result<IngestedFile> {
    ingest_file_inner(copied.path().to_owned(), name, host, copied.copied_digest(), false, async |_| Ok(None), async |_| Ok(None)).await
}

/// A copied checksum or a previously committed file revision can reuse the
/// same ingestion checks and inspection cache without re-reading all bytes.
pub async fn ingest_verified_file(path: PathBuf, name: String, host: &dyn cpu_host::CpuHost, digest: (ContentHash, u64, u64), cached: impl AsyncFnOnce(BookId) -> io::Result<Option<InspectedBook>>) -> io::Result<IngestedFile> {
    ingest_file_inner(path, name, host, Some(digest), false, async |_| Ok(None), cached).await
}

/// Reuse a current inspection only after hashing the actual source bytes.
pub async fn ingest_file_with_cached_inspection(path: PathBuf, name: String, host: &dyn cpu_host::CpuHost, cached: impl AsyncFnOnce(BookId) -> io::Result<Option<InspectedBook>>) -> io::Result<IngestedFile> {
    ingest_file_inner(path, name, host, None, false, async |_| Ok(None), cached).await
}

/// Reuse independently versioned reader facts after hashing the source bytes.
pub async fn ingest_file_with_inspection_caches(
    path: PathBuf, name: String, host: &dyn cpu_host::CpuHost, cached_pdf: impl AsyncFnOnce(BookId) -> io::Result<Option<CachedPdfReader>>, cached: impl AsyncFnOnce(BookId) -> io::Result<Option<InspectedBook>>,
) -> io::Result<IngestedFile> {
    ingest_file_inner(path, name, host, None, false, cached_pdf, cached).await
}

/// Reader imports retain external bibliographic facts and only require EPUB
/// package/navigation parsing. All byte and file-revision checks are shared.
pub async fn ingest_reader_file_with_cached_inspection(
    path: PathBuf, name: String, host: &dyn cpu_host::CpuHost, verified: Option<(ContentHash, u64, u64)>, cached: impl AsyncFnOnce(BookId) -> io::Result<Option<InspectedBook>>,
) -> io::Result<IngestedFile> {
    ingest_file_inner(path, name, host, verified, true, async |_| Ok(None), cached).await
}

async fn ingest_file_inner(
    path: PathBuf, name: String, host: &dyn cpu_host::CpuHost, copied: Option<(ContentHash, u64, u64)>, reader_metadata: bool, cached_pdf: impl AsyncFnOnce(BookId) -> io::Result<Option<CachedPdfReader>>,
    cached: impl AsyncFnOnce(BookId) -> io::Result<Option<InspectedBook>>,
) -> io::Result<IngestedFile> {
    let format = BookFormat::from_file_name(&name).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unsupported book format"))?;
    let permit = executor::inspection_permit().await;
    let (book_id, size_bytes, fingerprint, source_identity, reader) = executor::run_blocking({
        let path = path.clone();
        move || -> io::Result<_> {
            let _permit = permit;
            let fingerprint = fingerprint(&path)?;
            let mut reader = library_files::filesystem::open(&path)?;
            let (checksum, size_bytes) = match copied {
                Some((checksum, size, revision)) => {
                    if revision != fingerprint {
                        return Err(io::Error::new(io::ErrorKind::Interrupted, "copied book changed before inspection"));
                    }
                    (checksum, size)
                }
                None => hash_reader_with_size(&mut reader)?,
            };
            if size_bytes == 0 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "empty book"));
            }
            require_unchanged(&path, fingerprint)?;
            let book_id = BookId::from(checksum);
            reader.rewind()?;
            let source_identity = if format == BookFormat::Pdf {
                let identity = book_inspection::PdfSourceIdentity::read(&mut reader).ok().flatten();
                reader.rewind()?;
                identity
            } else {
                None
            };
            Ok((book_id, size_bytes, fingerprint, source_identity, reader))
        }
    })
    .await
    .map_err(|error| io::Error::other(error.to_string()))??;
    let inspection = match cached(book_id).await? {
        Some(inspection) => inspection,
        None if reader_metadata && format == BookFormat::Epub => cpu_host::submit_book(host, cpu_host::InspectEpubMetadata { source_name: name }, cpu_host::as_cpu_reader(reader)).await.map_err(io::Error::other)?,
        None => cpu_host::submit_book(host, cpu_host::Inspect { source_name: name, format, book_id: Some(book_id), cached_pdf: cached_pdf(book_id).await? }, cpu_host::as_cpu_reader(reader)).await.map_err(io::Error::other)?,
    };
    // A file edited during inspection must be retried rather than committed
    // with a content_hash and metadata from different revisions.
    require_unchanged(&path, fingerprint)?;
    Ok(IngestedFile { source_identity, book_id, size_bytes, fingerprint, format, inspection })
}

/// Inspect the reproducible archive of a native MP3 folder, while keeping its
/// tracks and sidecars as ordinary files at the library placement.
pub async fn ingest_audiobook_folder(path: PathBuf, name: String, host: &dyn cpu_host::CpuHost) -> io::Result<IngestedFile> {
    let permit = executor::inspection_permit().await;
    let (book_id, size_bytes, fingerprint, reader) = executor::run_blocking({
        let path = path.clone();
        move || -> io::Result<_> {
            let _permit = permit;
            let tracks = audiobook_folder::discover(&path)?;
            if tracks.is_empty() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook folder has no MP3 tracks"));
            }
            let fingerprint = library_files::filesystem::audiobook_fingerprint(&path, &tracks)?;
            require_folder_unchanged(&path, fingerprint)?;
            let mut archive = audiobook_folder::write_folder_archive(&path, tempfile::tempfile()?, &tracks)?;
            require_folder_unchanged(&path, fingerprint)?;
            archive.rewind()?;
            let (checksum, size_bytes) = hash_reader_with_size(&mut archive)?;
            archive.rewind()?;
            Ok((BookId::from(checksum), size_bytes, fingerprint, archive))
        }
    })
    .await
    .map_err(|error| io::Error::other(error.to_string()))??;
    let format = BookFormat::Mp3Folder;
    let inspection = cpu_host::submit_book(host, cpu_host::Inspect { source_name: name, format, book_id: Some(book_id), cached_pdf: None }, cpu_host::as_cpu_reader(reader)).await;
    require_folder_unchanged(&path, fingerprint)?;
    Ok(IngestedFile { source_identity: None, book_id, size_bytes, fingerprint, format, inspection: inspection.map_err(io::Error::other)? })
}

fn require_folder_unchanged(path: &Path, fingerprint: u64) -> io::Result<()> {
    let tracks = audiobook_folder::discover(path)?;
    if library_files::filesystem::audiobook_fingerprint(path, &tracks)? != fingerprint {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "audiobook folder changed during inspection"));
    }
    Ok(())
}

fn require_unchanged(path: &Path, fingerprint: u64) -> io::Result<()> {
    if self::fingerprint(path)? != fingerprint {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "book changed during ingestion"));
    }
    Ok(())
}

pub(crate) fn fingerprint(path: &Path) -> std::io::Result<u64> {
    let audio = library_files::filesystem::fingerprint(path)?;
    if !path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("m4b")) {
        return Ok(audio);
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(&audio.to_le_bytes());
    if let Some(cover) = audiobook_folder::discover_m4b_cover(path)? {
        hasher.update(cover.name.as_bytes());
        hasher.update(&library_files::filesystem::fingerprint(cover.path)?.to_le_bytes());
    }
    let [a, b, c, d, e, f, g, h, ..] = *hasher.finalize().as_bytes();
    Ok(u64::from_le_bytes([a, b, c, d, e, f, g, h]))
}
