use sync_common::ContentHash;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BlobKind {
    Book,
    Thumbnail,
}

impl BlobKind {
    pub const fn storage(self) -> &'static str {
        match self {
            Self::Book => "book",
            Self::Thumbnail => "thumbnail",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BlobKey {
    kind: BlobKind,
    content_hash: ContentHash,
}

impl BlobKey {
    pub fn new(kind: BlobKind, content_hash: ContentHash) -> Self {
        Self { kind, content_hash }
    }
    pub const fn kind(&self) -> BlobKind {
        self.kind
    }
    pub fn content_hash(&self) -> &ContentHash {
        &self.content_hash
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TransferJobKind {
    UploadBook,
    UploadThumbnail,
    DownloadBook,
    DownloadThumbnail,
}

impl TransferJobKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::UploadBook => "Upload book",
            Self::UploadThumbnail => "Upload thumbnail",
            Self::DownloadBook => "Download book",
            Self::DownloadThumbnail => "Download thumbnail",
        }
    }
    pub const fn storage(self) -> &'static str {
        match self {
            Self::UploadBook => "upload_blob",
            Self::UploadThumbnail => "upload_thumbnail",
            Self::DownloadBook => "download_blob",
            Self::DownloadThumbnail => "download_thumbnail",
        }
    }
    pub fn parse_storage(value: &str) -> Option<Self> {
        match value {
            "upload_blob" => Some(Self::UploadBook),
            "upload_thumbnail" => Some(Self::UploadThumbnail),
            "download_blob" => Some(Self::DownloadBook),
            "download_thumbnail" => Some(Self::DownloadThumbnail),
            _ => None,
        }
    }
    pub const fn blob_kind(self) -> BlobKind {
        match self {
            Self::UploadBook | Self::DownloadBook => BlobKind::Book,
            Self::UploadThumbnail | Self::DownloadThumbnail => BlobKind::Thumbnail,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TransferState {
    Queued,
    Running,
    Retrying,
}

impl TransferState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Retrying => "Retrying",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TransferOrigin {
    #[default]
    Background,
    UserInitiated,
}

impl TransferOrigin {
    pub const fn storage(self) -> &'static str {
        match self {
            Self::Background => "background",
            Self::UserInitiated => "user_initiated",
        }
    }
    pub fn parse_storage(value: &str) -> Option<Self> {
        match value {
            "background" => Some(Self::Background),
            "user_initiated" => Some(Self::UserInitiated),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NetworkTransport {
    Offline,
    Wifi,
    Cellular,
    Ethernet,
    Vpn,
    Other,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct NetworkStatus {
    pub transport: NetworkTransport,
    pub connected: bool,
    pub metered: Option<bool>,
    pub validated: Option<bool>,
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self { transport: NetworkTransport::Unknown, connected: true, metered: None, validated: None }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct TransferStatus {
    pub id: String,
    pub kind: TransferJobKind,
    pub content_hash: String,
    pub book_id: Option<String>,
    pub path: Option<String>,
    pub state: TransferState,
    pub origin: TransferOrigin,
    pub attempts: u32,
    pub last_error: Option<String>,
    pub next_retry_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct LibraryTransfers {
    pub library_id: String,
    pub library_name: String,
    pub transfers: Vec<TransferStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistence_values_are_closed_and_round_trip() {
        for kind in [TransferJobKind::UploadBook, TransferJobKind::UploadThumbnail, TransferJobKind::DownloadBook, TransferJobKind::DownloadThumbnail] {
            assert_eq!(TransferJobKind::parse_storage(kind.storage()), Some(kind));
        }
        assert_eq!(TransferJobKind::parse_storage("upload"), None);
    }
}
