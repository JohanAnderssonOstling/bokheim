//! Serializable book sources produced by a library session.

use book_access::RemoteFile;
use library_model::PlaybackMetadata;
use remote_book_access::DirectPlayback;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ResolvedBookData {
    pub pdf_metadata: Option<pdf_view_common::PdfReaderMetadata>,
    pub format: book_model::BookFormat,
    pub source: BookSource,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum BookSource {
    Local {
        capability: String,
        length: u64,
    },
    Remote {
        source: RemoteFile,
        #[serde(with = "serde_bytes")]
        prefix: Vec<u8>,
    },
    /// Bokheim ZIP-based books start with the tail; the reader plans entry ranges.
    RemoteArchive {
        source: RemoteFile,
        #[serde(with = "serde_bytes")]
        tail: Vec<u8>,
    },
    Audiobook(PlaybackSource),
    /// Native wire adapters can still return owned bytes; browsers use capabilities.
    Inline(#[serde(with = "serde_bytes")] Vec<u8>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PlaybackLocation {
    LocalUrl(String),
    LocalTracks(Vec<String>),
    Direct(DirectPlayback),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaybackSource {
    pub metadata: PlaybackMetadata,
    pub location: PlaybackLocation,
}
