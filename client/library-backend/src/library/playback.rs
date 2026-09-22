//! Values exchanged with a running library session.
//!
//! They intentionally contain no application, executor, storage, or account
//! objects.

use serde::{Deserialize, Serialize};
use sync_common::ContentHash;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaybackMetadata {
    pub title: String,
    pub author: String,
    pub narrator: Option<String>,
    pub duration_ms: u64,
    pub chapters: Vec<library_model::AudiobookChapter>,
    #[serde(with = "serde_bytes")]
    pub cover: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DirectPlayback {
    pub url: String,
    pub expires_in_seconds: u32,
    pub checksum: ContentHash,
    pub length: u64,
}

impl std::fmt::Debug for DirectPlayback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectPlayback").field("checksum", &self.checksum).finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum PlaybackLocation {
    LocalUrl(String),
    Direct(DirectPlayback),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaybackSource {
    pub metadata: PlaybackMetadata,
    pub location: PlaybackLocation,
}
