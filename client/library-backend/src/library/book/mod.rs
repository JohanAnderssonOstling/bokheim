//! Shared book-opening policy. Hosts provide local readers and supported
//! streaming mechanisms; metadata selection and remote access are common.

use super::remote_access::RemoteBookAccess;
use crate::BackendError;
use crate::BoxedBookReader;
use crate::{BookId, LibrarySession};
use book_content::BookFormat as TextBookFormat;
use book_model::BookFormat;
use client_platform_runtime::startup_profile::StartupProfile;
use library_database::LocalBookInfo;

#[cfg(target_arch = "wasm32")]
pub(crate) mod browser;

#[cfg(not(target_arch = "wasm32"))]
#[path = "native.rs"]
mod host;
#[cfg(target_arch = "wasm32")]
#[path = "web.rs"]
mod host;
mod remote_archive;
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
}
impl DocumentBook {
    pub fn new(path: String, format: DocumentFormat, reader: BoxedBookReader) -> Self {
        Self { path, format, source: DocumentSource::Reader(reader) }
    }
}

/// Text books and comics share asynchronous sources; PDF retains its seekable input.
pub enum DocumentSource {
    Reader(BoxedBookReader),
    Async(book_access::AsyncBookSource),
}

impl DocumentBook {
    pub fn from_async_source(path: String, format: DocumentFormat, source: book_access::AsyncBookSource) -> Self {
        Self { path, format, source: DocumentSource::Async(source) }
    }
}

struct RemoteDocument {
    source: crate::library::RemoteFile,
    access: RemoteBookAccess,
    prefix: Vec<u8>,
    pdf_metadata: Option<pdf_view_common::PdfReaderMetadata>,
}
async fn remote_document(library: &LibrarySession, hash: BookId) -> Result<RemoteDocument, BackendError> {
    let (source, prefix) = library.describe_remote_file(hash).await.map_err(BackendError::operation)?;
    let pdf_metadata = library.db.pdf_metadata(&hash)?;
    let access = library.remote_book_access(&source)?;
    Ok(RemoteDocument { source, access, prefix, pdf_metadata })
}

impl LibrarySession {
    pub(crate) async fn prepare_book_for_reader(&self, hash: BookId) -> Result<Opened, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        if self.is_opds() {
            return self.open_opds_book(hash).await;
        }
        let mut profile = StartupProfile::new("book_lookup_resolve");
        let info = self.db.local_book_info(&hash)?;
        profile.mark("local_book_info");
        if let Some(book) = host::local(self, hash, &info).await? {
            profile.mark("local_source");
            return Ok(book);
        }

        self.account()?.require().map_err(|error| BackendError::classified(client_runtime::Failure::SignInRequired, error.to_string()))?;

        host::remote(self, hash, &info).await
    }
}

#[cfg(target_arch = "wasm32")]
pub mod web_reader;
