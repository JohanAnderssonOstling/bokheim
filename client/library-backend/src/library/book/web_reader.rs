//! Browser book-opening policy over typed source capabilities.

use std::future::Future;

use crate::library::PlaybackLocation::Direct;
use crate::library::{DirectPlayback, DocumentFormat, PdfBook, PlaybackSource, RemoteFile, ResolvedBook};
use crate::BookId;
pub use book_access::remote_file::BrowserReader as Reader;
use book_access::{FetchFuture, SparseBookSource};
use book_model::BookFormat;
pub use client_platform_web::transport::range::connect;

use crate::{BackendError, BookSource, ResolvedBookData};
use client_protocol::{Failure, OperationError};

/// Library-scoped source operations supplied by the application host. Each
/// operation fixes its result type; wire commands stay inside the adapter.
pub trait BookTransport: Clone + 'static {
    fn open_remote_audio(&self, book: BookId) -> impl Future<Output = Result<Option<PlaybackSource>, BackendError>>;
    fn renew_remote_audio(&self, book: BookId) -> impl Future<Output = Result<DirectPlayback, BackendError>>;
    fn read_book_range(&self, capability: String, offset: u64, length: usize) -> impl Future<Output = Result<Vec<u8>, BackendError>>;
    fn read_file_range(&self, source: RemoteFile, offset: u64, length: usize) -> impl Future<Output = Result<Vec<u8>, BackendError>>;
    fn read_file_ranges(&self, source: RemoteFile, ranges: Vec<(u64, usize)>) -> impl Future<Output = Result<Vec<u8>, BackendError>>;
    fn subscribe_book(&self, book_id: BookId) -> impl Future<Output = Result<(async_channel::Receiver<()>, ResolvedBookData), BackendError>>;
}

#[derive(Clone)]
enum RangeSource {
    Local(String),
    Remote(RemoteFile),
}

/// Keep the subscription alive for every in-flight read as well as the fetcher.
fn range_fetch<T: BookTransport>(library: T, lease: Option<async_channel::Receiver<()>>, source: RangeSource) -> impl Fn(u64, usize) -> FetchFuture<'static, Result<Vec<u8>, OperationError>> {
    move |offset, length| {
        let library = library.clone();
        let lease = lease.clone();
        let source = source.clone();
        Box::pin(async move {
            let _lease = lease;
            match source {
                RangeSource::Local(capability) => library.read_book_range(capability, offset, length).await,
                RangeSource::Remote(source) => library.read_file_range(source, offset, length).await,
            }
            .map_err(|error| OperationError::from_classified(&error))
        })
    }
}

async fn audiobook<T: BookTransport>(source: crate::library::PlaybackSource, lease: Option<async_channel::Receiver<()>>, library: T, book_id: BookId) -> Result<ResolvedBook, BackendError> {
    let renewal_library = library.clone();
    let renew = matches!(&source.location, Direct(_)).then(|| {
        client_platform_web::transport::resources::AsyncCallback::new(move || {
            let library = renewal_library.clone();
            async move {
                let direct = library.renew_remote_audio(book_id).await.map_err(|error| OperationError::from_classified(&error).into_js())?;
                let json = serde_json::to_string(&direct).map_err(|error| OperationError::operation(error).context("Invalid direct playback configuration").into_js())?;
                js_sys::JSON::parse(&json)
            }
        })
    });
    let (url, direct, local_tracks) = match source.location {
        crate::library::PlaybackLocation::LocalUrl(url) => (url, None, Vec::new()),
        crate::library::PlaybackLocation::LocalTracks(tracks) => (String::new(), None, tracks),
        Direct(direct) => (direct.url.clone(), Some(direct), Vec::new()),
    };
    Ok(ResolvedBook::Audiobook(crate::library::browser_audiobook::BrowserAudiobook::new(source.metadata, url, local_tracks, direct, lease, renew)))
}

pub async fn resolve_book<T: BookTransport>(library: &T, book_id: BookId) -> Result<ResolvedBook, BackendError> {
    if let Some(source) = library.open_remote_audio(book_id).await? {
        return audiobook(source, None, library.clone(), book_id).await;
    }
    let (lease, resolved) = crate::executor::timeout(std::time::Duration::from_secs(65), library.subscribe_book(book_id)).await.map_err(|_| BackendError::classified(Failure::TimedOut, "opening the book source timed out"))??;
    if let Ok(document_format) = DocumentFormat::try_from(resolved.format) {
        let remote = matches!(&resolved.source, BookSource::Remote { .. } | BookSource::RemoteArchive { .. });
        let source = match resolved.source {
            BookSource::Local { capability, length } => {
                let fetch = range_fetch(library.clone(), Some(lease), RangeSource::Local(capability));
                book_access::AsyncBookSource::browser(length, Vec::new(), fetch)?
            }
            BookSource::RemoteArchive { source, tail } if matches!(resolved.format, BookFormat::Epub | BookFormat::Cbz) => {
                let fetch = range_fetch(library.clone(), None, RangeSource::Remote(source.clone()));
                let seed_offset = source.length.checked_sub(tail.len() as u64).ok_or("invalid ZIP tail")?;
                let sparse = SparseBookSource::browser(source.length, seed_offset, tail, fetch)?;
                let batch_library = library.clone();
                let batch_source = source.clone();
                let sparse = sparse.with_browser_prefetch(move |ranges| {
                    let library = batch_library.clone();
                    let source = batch_source.clone();
                    Box::pin(async move { library.read_file_ranges(source, ranges).await.map_err(|error| OperationError::from_classified(&error)) })
                });
                document_format.prepare_archive(sparse).await?
            }
            BookSource::Remote { source, prefix } => {
                let length = source.length;
                let fetch = range_fetch(library.clone(), Some(lease), RangeSource::Remote(source));
                book_access::AsyncBookSource::browser(length, prefix, fetch)?
            }
            _ => return Err("browser received an invalid book source".into()),
        };
        let document = if remote { crate::library::DocumentBook::from_remote_source(String::new(), document_format, source) } else { crate::library::DocumentBook::from_async_source(String::new(), document_format, source) };
        return Ok(ResolvedBook::Document(document));
    }
    let reader: crate::BoxedBookReader = match resolved.source {
        BookSource::Audiobook(source) => return audiobook(source, Some(lease), library.clone(), book_id).await,
        BookSource::Local { capability, length } => {
            let fetch = range_fetch(library.clone(), Some(lease), RangeSource::Local(capability));
            book_access::remote_file::browser_reader(length, Vec::new(), fetch)?
        }
        BookSource::Remote { source, prefix } => {
            let library = library.clone();
            if resolved.format == BookFormat::Pdf {
                let length = source.length;
                let fetch = book_access::remote_file::browser_bundle_fetch(move |ranges| {
                    let library = library.clone();
                    let source = source.clone();
                    Box::pin(async move { library.read_file_ranges(source, ranges).await.map_err(|error| OperationError::from_classified(&error)) })
                })?;
                book_access::pdf_bundle::open(length, prefix, fetch).map_err(|error| OperationError::from_classified(&error))?
            } else {
                let length = source.length;
                let fetch = range_fetch(library, None, RangeSource::Remote(source));
                book_access::remote_file::browser_reader(length, prefix, fetch)?
            }
        }
        BookSource::RemoteArchive { .. } => return Err("browser received a ZIP source for another format".into()),
        BookSource::Inline(_) => return Err("browser received bytes instead of a book source".into()),
    };
    if resolved.format != BookFormat::Pdf {
        return Err(BackendError::message("browser received a synchronous source for an unsupported document format"));
    }
    Ok(ResolvedBook::Pdf(PdfBook { path: String::new(), reader, metadata: resolved.pdf_metadata }))
}
