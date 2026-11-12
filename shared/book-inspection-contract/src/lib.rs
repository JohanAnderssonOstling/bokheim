//! Inspection results shared by extractors, server ingestion, and client persistence.
//! No parsers, file handles, or inspection-version policy live here.
use book_model::{AudiobookChapter, BookRecord, BookTocEntry, ChapterOrigin, Contributor};
use content_address::{BookId, ContentHash};
use metadata_contract::audible::LookupRequest;
use pdf_view_common::PdfReaderMetadata;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AudiobookMetadata {
    pub title: String,
    pub authors: Vec<Contributor>,
    pub description: String,
    pub duration_ms: u64,
    pub chapters: Vec<AudiobookChapter>,
    #[serde(default)]
    pub tracks: Vec<book_model::AudiobookTrack>,
    pub chapter_origin: ChapterOrigin,
    #[serde(default)]
    pub recording: metadata_contract::audible::RecordingEvidence,
}

impl AudiobookMetadata {
    /// Original recording evidence used for lookup and enrichment invalidation.
    pub fn into_audible_lookup_request(self) -> LookupRequest {
        LookupRequest { title: self.title, authors: self.authors.iter().map(|author| author.name().to_owned()).collect(), duration_ms: self.duration_ms, recording: self.recording, chapters: self.chapters }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct InspectedBook {
    #[serde(default)]
    pub pdf: Option<PdfReaderMetadata>,
    pub metadata: BookRecord,
    pub audiobook: Option<AudiobookMetadata>,
    /// The book's own navigation document, kept rather than discarded:
    /// the detail page draws contents for books that have never been
    /// downloaded, and re-reading the file for every visit is not an option
    /// when the file may be remote. Empty where the format carries none — an
    /// M4B's chapters are audiobook metadata, which the audiobook path files as
    /// navigation of its own.
    #[serde(default)]
    pub toc: Vec<BookTocEntry>,
}

/// Geometry and navigation remain valid when only bibliographic extraction changes.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CachedPdfReader {
    pub metadata: PdfReaderMetadata,
    pub toc: Vec<BookTocEntry>,
}

/// Optional PDF provenance. These values never replace hashing the full file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfSourceIdentity {
    pub book_id: BookId,
    pub canonical_hash: Option<ContentHash>,
}
