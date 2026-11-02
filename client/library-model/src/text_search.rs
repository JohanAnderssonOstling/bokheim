use crate::{BookCardRow, BrowsePageState};
pub use book_model::BookTextMatch;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookTextSearchBook {
    pub book: BookCardRow,
    pub match_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BookTextSearchPage {
    pub books: Vec<BookTextSearchBook>,
    pub page: BrowsePageState,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BookTextMatchPage {
    pub matches: Vec<BookTextMatch>,
    pub offset: usize,
    pub total: usize,
}

/// Positions removed from a retained search snapshot, before this removal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextSearchRemoval {
    pub revision: String,
    pub positions: Vec<usize>,
}
