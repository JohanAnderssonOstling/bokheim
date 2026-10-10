//! Shared transfer values.
use content_address::ContentHash;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferJobKind {
    UploadBook,
    UploadThumbnail,
    DownloadBook,
    DownloadThumbnail,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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

