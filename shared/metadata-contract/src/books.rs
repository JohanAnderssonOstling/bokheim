//! One request per book. The client sends what the book's file says; the
//! server runs every lookup and merge rule and returns the book's final
//! record. The client keeps no enrichment state beyond the last answer.
use serde::{Deserialize, Serialize};

pub const MAX_BOOKS_PER_REQUEST: usize = 8;
pub const MAX_BOOKS_REQUEST_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BookEnrichmentRequest {
    pub books: Vec<BookFacts>,
}

/// File facts: the synchronized metadata register, bounded page evidence
/// included, plus local evidence that is never synchronized.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BookFacts {
    /// Opaque correlation key, echoed back. Not a library identifier.
    pub key: String,
    /// `description` is sent empty: the file's own description always wins,
    /// so the server only needs to report what it found.
    pub record: book_model::BookRecord,
    pub audiobook: bool,
    /// Other ways to read the book's title and local filenames, parsed on the
    /// device and unconfirmed. The server tries each as an alternative lookup.
    #[serde(default)]
    pub readings: Vec<TitleReading>,
    #[serde(default)]
    pub recording: Option<crate::audible::LookupRequest>,
}

/// One reading of a title or filename. `surname` is set instead of
/// `authors` when only a surname was written, as in `Smith_Some_Title`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TitleReading {
    pub title: String,
    pub authors: Vec<String>,
    pub surname: Option<String>,
    pub year: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BookEnrichmentResponse {
    /// Changes whenever lookup or merge rules change.
    pub policy: String,
    pub results: Vec<BookEnrichment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BookEnrichment {
    pub key: String,
    pub status: BookEnrichmentStatus,
    /// The file facts with everything the server resolved merged in. Present
    /// when `status` is `Found`. `description` is the server's description,
    /// empty when none was found.
    pub record: Option<book_model::BookRecord>,
    /// An audiobook's chapters from its matched recording.
    #[serde(default)]
    pub toc: Option<Vec<book_model::BookTocEntry>>,
    #[serde(default)]
    pub cover: Option<CoverSource>,
    #[serde(default)]
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BookEnrichmentStatus {
    Found,
    NoMatch,
    /// A lookup failed; ask again soon.
    Failed,
}

/// Confirmed artwork fetched from the metadata server at `/v3/covers/{id}`.
/// Covers are set once, so this only fills a book that has none.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CoverSource {
    pub id: String,
}
