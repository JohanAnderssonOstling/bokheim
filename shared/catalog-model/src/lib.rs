//! Small, transport-independent catalog values used by clients and servers.

mod author;
mod search;

pub use author::{AuthorName, AuthorNameError, AuthorNameMatchKey};
pub use search::{normalize_search_text, normalized_substring_match};
