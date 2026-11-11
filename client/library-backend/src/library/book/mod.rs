//! Book opening, reader access, annotations, and reading state.
//!
//! Hosts provide local readers and supported
//! streaming mechanisms; metadata selection and remote access are common.

use crate::book::{BookSource, ResolvedBookData};
use crate::library::LibraryResources;
use crate::BackendError;
use crate::BookId;
use crate::BoxedBookReader;
use book_access::{AsyncBookSource, SparseBookSource};
use book_content::BookFormat as TextBookFormat;
use book_model::BookFormat;
use library_database::LocalBookInfo;
use library_placement::LibraryState;
use remote_book_access::{describe_archive, describe_remote_file};

mod annotations;
mod progress;
pub(super) mod reading;
mod reads;
mod text_cache;
pub use text_cache::TextCache;

#[cfg(target_arch = "wasm32")]
pub(crate) mod browser;

#[cfg(not(target_arch = "wasm32"))]
#[path = "native.rs"]
mod host;
#[cfg(target_arch = "wasm32")]
#[path = "web.rs"]
mod host;
pub use host::AudiobookBook;
pub(crate) use host::Opened;

/// Opening a document and opening audio produce different usable resources.
pub enum ResolvedBook {
    Document(DocumentBook),
    Pdf(PdfBook),
    Audiobook(AudiobookBook),
}
impl ResolvedBook {
    pub fn format(&self) -> BookFormat {
        match self {
            Self::Document(book) => book.format.into(),
            Self::Pdf(_) => BookFormat::Pdf,
            #[cfg(not(target_arch = "wasm32"))]
            Self::Audiobook(book) => book.format,
            #[cfg(target_arch = "wasm32")]
            Self::Audiobook(book) => book.metadata.format,
        }
    }
    pub fn into_document(self) -> Result<DocumentBook, String> {
        match self {
            Self::Document(book) => Ok(book),
            Self::Pdf(_) | Self::Audiobook(_) => Err("expected a text or comic document".into()),
        }
    }

    pub fn into_pdf(self) -> Result<PdfBook, String> {
        match self {
            Self::Pdf(book) => Ok(book),
            Self::Document(_) | Self::Audiobook(_) => Err("expected a PDF document".into()),
        }
    }
}

/// Formats whose content can be supplied by either kind of document source.
/// PDF and audio have their own resource types and cannot enter this set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentFormat {
    Text(TextBookFormat),
    Comic,
}

impl DocumentFormat {
    async fn prepare_archive(self, source: SparseBookSource) -> Result<AsyncBookSource, BackendError> {
        match self {
            Self::Text(TextBookFormat::Epub) => book_access::prepare_epub(source).await.map_err(BackendError::operation),
            Self::Comic => Ok(source.into()),
            _ => Err(BackendError::message("expected an EPUB or comic archive")),
        }
    }
}

impl TryFrom<BookFormat> for DocumentFormat {
    type Error = BackendError;

    fn try_from(format: BookFormat) -> Result<Self, Self::Error> {
        match format {
            BookFormat::Epub => Ok(Self::Text(TextBookFormat::Epub)),
            BookFormat::Mobi => Ok(Self::Text(TextBookFormat::Mobi)),
            BookFormat::Fb2 => Ok(Self::Text(TextBookFormat::Fb2)),
            BookFormat::Cbz => Ok(Self::Comic),
            BookFormat::Pdf | BookFormat::M4b | BookFormat::Mp3Folder => Err(BackendError::message("expected a text or comic document format")),
        }
    }
}

impl From<DocumentFormat> for BookFormat {
    fn from(format: DocumentFormat) -> Self {
        match format {
            DocumentFormat::Text(TextBookFormat::Epub) => Self::Epub,
            DocumentFormat::Text(TextBookFormat::Mobi) => Self::Mobi,
            DocumentFormat::Text(TextBookFormat::Fb2) => Self::Fb2,
            DocumentFormat::Comic => Self::Cbz,
        }
    }
}

/// The seekable PDF input and the metadata pinned to that same file revision.
pub struct PdfBook {
    pub path: String,
    pub reader: BoxedBookReader,
    pub metadata: Option<pdf_view_common::PdfReaderMetadata>,
}

pub struct DocumentBook {
    pub path: String,
    pub format: DocumentFormat,
    pub source: DocumentSource,
    remote: bool,
}
impl DocumentBook {
    pub fn new(path: String, format: DocumentFormat, reader: BoxedBookReader) -> Self {
        Self { path, format, source: DocumentSource::Reader(reader), remote: false }
    }
}

/// Text books and comics share asynchronous sources; PDF retains its seekable input.
pub enum DocumentSource {
    Reader(BoxedBookReader),
    Async(book_access::AsyncBookSource),
}

impl DocumentBook {
    pub fn from_async_source(path: String, format: DocumentFormat, source: book_access::AsyncBookSource) -> Self {
        Self { path, format, source: DocumentSource::Async(source), remote: false }
    }

    pub fn from_remote_source(path: String, format: DocumentFormat, source: book_access::AsyncBookSource) -> Self {
        Self { path, format, source: DocumentSource::Async(source), remote: true }
    }

    pub fn is_remote(&self) -> bool {
        self.remote
    }
}

impl LibraryResources {
    async fn remote_book_source(&self, storage: &LibraryState, hash: BookId, format: BookFormat) -> Result<ResolvedBookData, BackendError> {
        if matches!(format, BookFormat::Epub | BookFormat::Cbz) {
            let (source, tail) = describe_archive(self.sync.http_client(), &self.account, self.id, hash).await.map_err(BackendError::operation)?;
            return Ok(ResolvedBookData { pdf_metadata: None, format, source: BookSource::RemoteArchive { source, tail } });
        }
        let (source, prefix) = describe_remote_file(self.sync.http_client(), &self.account, self.id, hash).await.map_err(BackendError::operation)?;
        let pdf_metadata = storage.database().pdf_metadata(&hash).await?;
        Ok(ResolvedBookData { pdf_metadata, format, source: BookSource::Remote { source, prefix } })
    }

    pub(crate) async fn prepare_book_for_reader(&self, storage: &LibraryState, hash: BookId) -> Result<Opened, BackendError> {
        let info = storage.database().local_book_info(&hash).await?;
        if let Some(book) = host::local(self, storage, hash, &info).await? {
            return Ok(book);
        }

        self.account.require_session().await.map_err(|error| BackendError::classified(client_protocol::Failure::SignInRequired, error.to_string()))?;

        host::remote(self, storage, hash, &info).await
    }
}

#[cfg(target_arch = "wasm32")]
pub mod web_reader;
