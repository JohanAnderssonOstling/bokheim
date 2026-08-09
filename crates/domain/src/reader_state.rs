use identifiers::{ContentHash, UnixMillis};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BookmarkState {
    pub content_hash: ContentHash,
    pub cfi: String,
    pub label: String,
    pub created_at: UnixMillis,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BookLinkState {
    pub source_content_hash: ContentHash,
    pub target_content_hash: ContentHash,
    pub relation: String,
    pub note: String,
    pub created_at: UnixMillis,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}
