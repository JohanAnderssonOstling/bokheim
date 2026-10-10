//! A synchronous document-inspection view shared by container formats.

use crate::book::PreparedText;
use crate::epub::EpubSession;
use crate::resource_path::local_resource_uri;
use crate::{PreparedResources, TocEntry};
use futures_lite::future::block_on;
use std::io;

enum DocumentSource<'a> {
    Epub(&'a mut EpubSession),
    Prepared(PreparedResources),
}

/// Logical document paths and navigation, with bounded prefix reads.
/// EPUB documents remain in the archive; MOBI exposes its prepared virtual HTML.
pub struct DocumentInspection<'a> {
    documents: Vec<String>,
    toc: Vec<TocEntry>,
    source: DocumentSource<'a>,
}

impl<'a> DocumentInspection<'a> {
    pub(crate) fn from_epub(epub: &'a mut EpubSession, documents: Vec<String>, toc: Vec<TocEntry>) -> Self {
        Self { documents, toc, source: DocumentSource::Epub(epub) }
    }

    pub(crate) fn from_prepared(text: PreparedText) -> Self {
        Self { documents: text.resources.documents().to_vec(), toc: text.toc, source: DocumentSource::Prepared(text.resources) }
    }

    pub fn document_uris(&self) -> &[String] {
        &self.documents
    }

    pub fn toc(&self) -> &[TocEntry] {
        &self.toc
    }

    /// Read at most `max_bytes` without requiring a complete EPUB document.
    pub fn read_prefix(&mut self, uri: &str, max_bytes: u64) -> io::Result<Vec<u8>> {
        let uri = local_resource_uri("", uri)?;
        match &mut self.source {
            DocumentSource::Epub(epub) => block_on(epub.read_document_prefix(&uri, max_bytes)),
            DocumentSource::Prepared(resources) => resources.read_prefix(&uri, max_bytes),
        }
    }
}
