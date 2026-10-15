use content_address::ContentHash;
use serde::{Deserialize, Serialize};
use std::fmt;

/// One lifecycle for browser HTTP downloads and library-owned torrents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserDownloadPhase {
    Preparing,
    Downloading,
    Importing,
    Waiting,
    Completed,
    Failed,
    Dismissed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserDownloadUpdate {
    pub download: String,
    /// Transfer an existing notification to the resolved torrent's stable ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
    pub name: String,
    pub phase: BrowserDownloadPhase,
    #[serde(default)]
    pub percent: Option<u8>,
    #[serde(default)]
    pub message: Option<String>,
}

/// Source download observations before a catalog entry has a library file hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CatalogImportProgress {
    Waiting,
    Downloading { downloaded_bytes: u64, total_bytes: Option<u64> },
    Importing,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct DownloadProgress(f32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadProgressError;

impl fmt::Display for DownloadProgressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("download progress must be finite and between zero and one")
    }
}

impl std::error::Error for DownloadProgressError {}

impl DownloadProgress {
    pub const ZERO: Self = Self(0.0);

    pub fn new(value: f32) -> Result<Self, DownloadProgressError> {
        if value.is_finite() && (0.0..=1.0).contains(&value) { Ok(Self(value)) } else { Err(DownloadProgressError) }
    }

    pub fn from_ratio(completed: u64, total: u64) -> Self {
        if total == 0 { Self::ZERO } else { Self((completed as f32 / total as f32).clamp(0.0, 1.0)) }
    }

    pub const fn value(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum DownloadState {
    NotDownloaded,
    Queued,
    Downloading(DownloadProgress),
    Downloaded,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct BookDownloadStatus {
    pub content_hash: ContentHash,
    pub state: DownloadState,
}

impl BookDownloadStatus {
    pub fn new(content_hash: ContentHash, state: DownloadState) -> Self {
        Self { content_hash, state }
    }
}
