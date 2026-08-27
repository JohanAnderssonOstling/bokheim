//! Transport-independent catalog values used by applications, storage,
//! clients, and servers.

mod author;
mod catalog;
mod search;

pub use author::{AuthorName, AuthorNameError, AuthorNameMatchKey};
pub use catalog::*;
pub use search::{normalize_search_text, normalized_substring_match};
