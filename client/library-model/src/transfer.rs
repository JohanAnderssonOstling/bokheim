//! Shared transfer values.
use content_address::BookId;

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
    book_id: BookId,
}

impl BlobKey {
    pub fn new(kind: BlobKind, book_id: BookId) -> Self {
        Self { kind, book_id }
    }
    pub const fn kind(&self) -> BlobKind {
        self.kind
    }
    pub fn book_id(&self) -> &BookId {
        &self.book_id
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

