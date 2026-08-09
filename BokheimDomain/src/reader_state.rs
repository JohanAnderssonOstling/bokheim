use bokheim_identifiers::{BookId, UnixMillis};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BookmarkState {
    pub book_id: BookId,
    pub cfi: String,
    pub label: String,
    pub created_at: UnixMillis,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BookLinkState {
    pub source_book_id: BookId,
    pub target_book_id: BookId,
    pub relation: String,
    pub note: String,
    pub created_at: UnixMillis,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}
