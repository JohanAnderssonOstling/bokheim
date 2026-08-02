use crate::ContentHash;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BlobKind {
    Book,
    Thumbnail,
}

impl BlobKind {
    pub(crate) const fn storage(self) -> &'static str {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, specta::Type)]
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

    pub(crate) const fn storage(self) -> &'static str {
        match self {
            Self::UploadBook => "upload_blob",
            Self::UploadThumbnail => "upload_thumbnail",
            Self::DownloadBook => "download_blob",
            Self::DownloadThumbnail => "download_thumbnail",
        }
    }

    pub(crate) fn parse_storage(value: &str) -> Option<Self> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_values_round_trip_through_closed_enums() {
        assert_eq!(BlobKind::Book.storage(), "book");
        assert_eq!(BlobKind::Thumbnail.storage(), "thumbnail");
        for kind in [TransferJobKind::UploadBook, TransferJobKind::UploadThumbnail, TransferJobKind::DownloadBook, TransferJobKind::DownloadThumbnail] {
            assert_eq!(TransferJobKind::parse_storage(kind.storage()), Some(kind));
        }
        assert_eq!(TransferJobKind::parse_storage("upload"), None);
    }
}
