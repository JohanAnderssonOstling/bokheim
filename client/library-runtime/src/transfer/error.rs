use std::fmt;

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TransferError {
    AuthenticationRequired,
    Retryable(String),
    Rejected(String),
}

impl TransferError {
    pub fn retryable(error: impl fmt::Display) -> Self { Self::Retryable(error.to_string()) }
    pub fn rejected(message: impl Into<String>) -> Self { Self::Rejected(message.into()) }
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
        match self { Self::AuthenticationRequired => formatter.write_str("authentication access token was rejected"), Self::Retryable(message) | Self::Rejected(message) => formatter.write_str(message) }
    }
}
impl std::error::Error for TransferError {}
