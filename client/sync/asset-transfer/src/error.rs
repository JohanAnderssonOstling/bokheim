//! Stable transfer failures shared by execution, queues and browser RPC.
use std::fmt;

const AUTHENTICATION_REQUIRED_MESSAGE: &str = "authentication access token was rejected";

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransferError {
    AuthenticationRequired,
    Retryable(String),
    Rejected(String),
}

impl TransferError {
    pub fn retryable(error: impl fmt::Display) -> Self {
        Self::Retryable(error.to_string())
    }

    pub fn rejected(message: impl Into<String>) -> Self {
        Self::Rejected(message.into())
    }

    /// Admission failures caused by the book or account are durable
    /// until the local file changes or the account's quota changes. An upload
    /// already in progress is different: another worker may release it, so it
    /// remains retryable.
    pub fn from_upload_admission(reason: sync_common::api::assets::BlobManifestRejectionReason) -> Self {
        use sync_common::api::assets::BlobManifestRejectionReason;
        let message = format!("server rejected upload admission: {reason:?}");
        match reason {
            BlobManifestRejectionReason::UploadInProgress => Self::retryable(message),
            BlobManifestRejectionReason::InvalidSize | BlobManifestRejectionReason::SizeMismatch | BlobManifestRejectionReason::IdentityMismatch | BlobManifestRejectionReason::QuotaExceeded => Self::rejected(message),
        }
    }
}

impl fmt::Display for TransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str(AUTHENTICATION_REQUIRED_MESSAGE),
            Self::Retryable(message) | Self::Rejected(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for TransferError {}

impl From<sync_transport::AssetResponseError> for TransferError {
    fn from(error: sync_transport::AssetResponseError) -> Self {
        match error {
            sync_transport::AssetResponseError::AuthenticationRequired => Self::AuthenticationRequired,
            sync_transport::AssetResponseError::StorageQuotaExceeded => Self::Retryable("book storage quota exceeded".to_owned()),
            sync_transport::AssetResponseError::Retryable(error) => Self::Retryable(error.to_string()),
            sync_transport::AssetResponseError::Rejected(error) => Self::Rejected(error.to_string()),
            sync_transport::AssetResponseError::Wire(error) => Self::retryable(error),
        }
    }
}

impl From<std::io::Error> for TransferError {
    fn from(error: std::io::Error) -> Self {
        Self::retryable(error)
    }
}

impl From<rusqlite::Error> for TransferError {
    fn from(error: rusqlite::Error) -> Self {
        Self::retryable(error)
    }
}

impl From<library_database::DatabaseError> for TransferError {
    fn from(error: library_database::DatabaseError) -> Self {
        Self::retryable(error)
    }
}

impl From<library_files::AssetStoreError> for TransferError {
    fn from(error: library_files::AssetStoreError) -> Self { Self::retryable(error) }
}
