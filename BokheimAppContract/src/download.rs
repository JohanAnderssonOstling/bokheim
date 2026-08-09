use serde::Serialize;
use specta::Type;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Type)]
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
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(DownloadProgressError)
        }
    }

    pub fn from_ratio(completed: u64, total: u64) -> Self {
        if total == 0 {
            Self::ZERO
        } else {
            Self((completed as f32 / total as f32).clamp(0.0, 1.0))
        }
    }

    pub const fn value(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub enum DownloadState {
    NotDownloaded,
    Queued,
    Downloading(#[specta(type = specta_typescript::Number)] DownloadProgress),
    Downloaded,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct BookDownloadStatus {
    pub content_hash: String,
    pub state: DownloadState,
}

impl BookDownloadStatus {
    pub fn new(content_hash: impl Into<String>, state: DownloadState) -> Self {
        Self { content_hash: content_hash.into(), state }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct FolderDownloadStatus {
    pub dir_id: String,
    #[specta(type = specta_typescript::Number)]
    pub downloaded_books: usize,
    #[specta(type = specta_typescript::Number)]
    pub total_books: usize,
}

impl FolderDownloadStatus {
    pub fn new(dir_id: impl Into<String>, downloaded_books: usize, total_books: usize) -> Self {
        Self { dir_id: dir_id.into(), downloaded_books, total_books }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_rejects_non_finite_and_out_of_range_values() {
        assert!(DownloadProgress::new(f32::NAN).is_err());
        assert!(DownloadProgress::new(-0.1).is_err());
        assert!(DownloadProgress::new(1.1).is_err());
        assert_eq!(DownloadProgress::new(0.5).unwrap().value(), 0.5);
        assert_eq!(DownloadProgress::from_ratio(3, 2).value(), 1.0);
    }
}
