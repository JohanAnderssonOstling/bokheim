use super::*;
use pdf_view_common::PdfStoredPage;
use std::io::{Error, Result};
use std::rc::Rc;

struct SharedReader<R>(Rc<RefCell<R>>);
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
    let book_id = hash.finalize().to_hex().to_string();
    inspect_reader_metadata_with_identity(reader, book_id)
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
    let shared = Rc::new(RefCell::new(reader));
    let session = LocalDocumentSession::from_reader("ingestion", SharedReader(shared.clone()))?;
    // PDFium's page-size query already applies the intrinsic page rotation.
    // No page contents, fonts, text characters, or annotations are loaded here.
    let pages = session.info.pages.iter().map(|info| PdfStoredPage { width: info.width_points, height: info.height_points, rotation: 0 }).collect();
    let outline = session.info.outline.clone();
    let metadata = PdfReaderMetadata { version: 1, book_id, pages, annotations: Vec::new() };
    drop(session);
    let reader = Rc::try_unwrap(shared).map_err(|_| PdfError::new("PDF ingestion reader still in use"))?.into_inner();
    Ok((reader, metadata, outline))
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
