use std::collections::HashSet;
use sync_common::{ContentHash, LibraryId};
use tokio::io::AsyncRead;

pub type AssetReader = Box<dyn AsyncRead + Unpin + Send>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Book,
    Thumbnail,
}

impl AssetKind {
    pub const fn max_bytes(self) -> u64 {
        match self {
            Self::Book => 1_000_000_000,
            Self::Thumbnail => sync_common::MAX_THUMBNAIL_BYTES,
        }
    }
}

pub struct StoredAsset {
    pub reader: AssetReader,
    pub total_len: u64,
    pub offset: u64,
}

/// A live cover is identified by both its owner and the book content it
/// represents. Book blobs remain globally content-addressed, but covers are
/// deliberately not shared across accounts.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UserThumbnail {
    pub user_id: String,
    pub content_hash: ContentHash,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveAssets {
    books: HashSet<ContentHash>,
    thumbnails: HashSet<UserThumbnail>,
}

impl LiveAssets {
    pub fn new(books: HashSet<ContentHash>, thumbnails: HashSet<UserThumbnail>) -> Self {
        Self { books, thumbnails }
    }

    pub fn books(&self) -> &HashSet<ContentHash> {
        &self.books
    }

    pub fn thumbnails(&self) -> &HashSet<UserThumbnail> {
        &self.thumbnails
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    pub scanned: usize,
    pub deleted: usize,
    pub bytes_reclaimed: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("asset access is forbidden")]
    Forbidden,
    #[error("invalid asset input: {0}")]
    InvalidInput(String),
    #[error("asset bytes do not match the requested content hash")]
    HashMismatch,
    #[error("book uploads require an exact Content-Length")]
    LengthRequired,
    #[error("asset body length {actual} does not match the declared length {expected}")]
    LengthMismatch { expected: u64, actual: u64 },
    #[error("an upload for this user and book is already in progress")]
    UploadInProgress,
    #[error("book upload staging capacity is temporarily exhausted")]
    Busy,
    #[error("book storage quota exceeded: limit={limit}, used={used}, reserved={reserved}, requested={requested}")]
    QuotaExceeded { limit: u64, used: u64, reserved: u64, requested: u64 },
    #[error("asset exceeds the {limit}-byte limit")]
    TooLarge { limit: u64 },
    #[error("asset byte offset {offset} is beyond its {len}-byte length")]
    RangeNotSatisfiable { offset: u64, len: u64 },
    #[error("asset storage failed: {0}")]
    Storage(String),
}

/// Persistence boundary used to authorize content-addressed assets without
/// exposing database technology to the application or HTTP layers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookUploadReservation {
    pub id: String,
    pub user_id: String,
    pub library_id: LibraryId,
    pub hash: ContentHash,
    pub expected_size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookUploadAdmission {
    AlreadyOwned,
    Reserved(BookUploadReservation),
}
