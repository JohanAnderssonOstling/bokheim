use super::*;
use pdf_view_common::PdfStoredPage;
use std::io::{Error, Result};
use std::rc::Rc;

struct SharedReader<R>(Rc<RefCell<R>>);

/// Reader facts and bounded opening-page text extracted from one PDF session.
pub struct PdfInspection {
    pub metadata: PdfReaderMetadata,
    pub outline: Vec<BookTocEntry>,
    pub page_text: Vec<(usize, String)>,
}
impl<R: Read> Read for SharedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        self.0.borrow_mut().read(buffer)
    }
}
impl<R: Seek> Seek for SharedReader<R> {
    fn seek(&mut self, position: SeekFrom) -> Result<u64> {
        self.0.borrow_mut().seek(position)
    }
}

/// Ingestion only: hashes the exact bytes and inspects page geometry/navigation.
/// Returns the original reader, rewound, without retaining a full-file copy.
/// The ingestion pass: the byte-specific snapshot a later open reuses, and the
/// book's navigation, which is neither byte-specific nor the renderer's
/// to keep. The outline is returned beside the snapshot rather than inside it
/// so a PDF reaches the library in the same navigation form an EPUB does.
pub fn inspect_reader_metadata<R: Read + Seek + Send + 'static>(mut reader: R) -> PdfResult<(R, PdfReaderMetadata, Vec<BookTocEntry>)> {
    let book_id = reader_identity(&mut reader)?;
    inspect_reader_metadata_with_identity(reader, book_id)
}

fn reader_identity(reader: &mut (impl Read + Seek)) -> PdfResult<String> {
    reader.seek(SeekFrom::Start(0)).map_err(PdfError::new_io)?;
    let mut hash = blake3::Hasher::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = match reader.read(&mut buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result.map_err(PdfError::new_io)?,
        };
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    reader.seek(SeekFrom::Start(0)).map_err(PdfError::new_io)?;
    Ok(hash.finalize().to_hex().to_string())
}

/// Inspect a source whose identity is already established by the caller.
/// The identity may be a verified content digest or a provider file identity;
/// this function does not calculate or verify a byte checksum. Provider
/// readers must pin one revision for the complete inspection, and callers
/// must invalidate cached metadata when that revision changes.
pub fn inspect_reader_metadata_with_identity<R: Read + Seek + Send + 'static>(mut reader: R, identity: String) -> PdfResult<(R, PdfReaderMetadata, Vec<BookTocEntry>)> {
    reader.seek(SeekFrom::Start(0)).map_err(PdfError::new_io)?;
    let book_id = identity;
    let (mut reader, mut metadata, outline) = runtime::run(move || inspect_local(reader, book_id))?;
    reader.seek(SeekFrom::Start(0)).map_err(PdfError::new_io)?;
    metadata.bound_optional_data();
    Ok((reader, metadata, outline))
}

fn inspect_local<R: Read + Seek + 'static>(reader: R, book_id: String) -> PdfResult<(R, PdfReaderMetadata, Vec<BookTocEntry>)> {
    let (reader, metadata, outline, ()) = inspect_local_with_text(reader, book_id, |_| Ok(()))?;
    Ok((reader, metadata, outline))
}

/// Reuse the PDFium session for geometry, navigation and bibliographic evidence.
/// A supplied identity skips hashing, as in `inspect_reader_metadata_with_identity`.
pub fn inspect_reader_with_text<R: Read + Seek + Send + 'static>(mut reader: R, identity: Option<String>, edge_pages: usize, max_text_bytes: usize) -> PdfResult<(R, PdfInspection)> {
    let book_id = match identity {
        Some(identity) => identity,
        None => reader_identity(&mut reader)?,
    };
    reader.rewind().map_err(PdfError::new_io)?;
    let (mut reader, mut metadata, outline, page_text) = runtime::run(move || inspect_local_with_text(reader, book_id, |document| inspect_document_page_text(document, edge_pages, max_text_bytes).map(|(_, pages)| pages)))?;
    reader.rewind().map_err(PdfError::new_io)?;
    metadata.bound_optional_data();
    Ok((reader, PdfInspection { metadata, outline, page_text }))
}

/// Inspect geometry/navigation and visit selected one-based pages in the same
/// PDFium session. The visitor can stop before any subsequent page is loaded.
pub fn inspect_reader_with_text_visitor<R: Read + Seek + Send + 'static, V: PdfPageTextVisitor>(
    mut reader: R, identity: Option<String>, pages: Vec<usize>, max_text_bytes: usize, mut visitor: V,
) -> PdfResult<(R, PdfReaderMetadata, Vec<BookTocEntry>, V)> {
    let book_id = match identity {
        Some(identity) => identity,
        None => reader_identity(&mut reader)?,
    };
    reader.rewind().map_err(PdfError::new_io)?;
    let (mut reader, mut metadata, outline, visitor) = runtime::run(move || {
        inspect_local_with_text(reader, book_id, |document| {
            visit_document_page_text(document, &pages, max_text_bytes, &mut visitor)?;
            Ok(visitor)
        })
    })?;
    reader.rewind().map_err(PdfError::new_io)?;
    metadata.bound_optional_data();
    Ok((reader, metadata, outline, visitor))
}

fn inspect_local_with_text<R: Read + Seek + 'static, T>(reader: R, book_id: String, inspect_text: impl FnOnce(&runtime::Document) -> PdfResult<T>) -> PdfResult<(R, PdfReaderMetadata, Vec<BookTocEntry>, T)> {
    let shared = Rc::new(RefCell::new(reader));
    let session = LocalDocumentSession::from_reader("ingestion", SharedReader(shared.clone()))?;
    let started = pdf_profile_start();
    // PDFium's page-size query already applies the intrinsic page rotation.
    // No page contents, fonts, text characters, or annotations are loaded here.
    let pages = session.info.pages.iter().map(|info| PdfStoredPage { width: info.width_points, height: info.height_points, rotation: 0 }).collect();
    let outline = session.info.outline.clone();
    let metadata = PdfReaderMetadata { version: 1, book_id, pages, annotations: Vec::new() };
    profile_pdf_phase("reader_metadata_assembly", 0, started);
    let page_text = inspect_text(&session.document)?;
    let started = pdf_profile_start();
    drop(session);
    profile_pdf_phase("document_close", 0, started);
    let reader = Rc::try_unwrap(shared).map_err(|_| PdfError::new("PDF ingestion reader still in use"))?.into_inner();
    Ok((reader, metadata, outline, page_text))
}

impl PdfError {
    fn new_io(error: Error) -> Self {
        Self::new(error.to_string())
    }
}

impl LocalDocumentSession {
    /// The caller supplies the content hash of the opened book.
    /// Invalid, old, or missing metadata uses normal inspection.
    pub fn from_reader_with_metadata<R: Read + Seek + 'static>(source_name: impl Into<String>, reader: R, book_id: &str, metadata: Option<PdfReaderMetadata>) -> PdfResult<Self> {
        let source_name = source_name.into();
        let Some(metadata) = metadata.filter(|m| m.valid_for(book_id)) else {
            return Self::from_reader(source_name, reader);
        };
        let document = load_document(reader).map_err(|e| PdfError::new(e.to_string()))?;
        if document.pages().len() as usize != metadata.pages.len() {
            return Self::from_document(source_name, document);
        }
        let pages = metadata.pages.into_iter().map(|p| PdfPageInfo { width_points: p.width, height_points: p.height }).collect();
        // Page geometry is cached because reading it costs one call per page.
        // The outline is not: it is one walk of a tree the document already
        // holds, and keeping a second copy of the library's navigation only to
        // save that walk is what this stopped being.
        let outline = document_outline(&document);
        let right_to_left = document.catalog().is_right_to_left();
        Ok(Self { document, info: PdfDocumentInfo { source_name, pages, outline, right_to_left }, analysis: RefCell::new(PageAnalysisCache::default()) })
    }
}
