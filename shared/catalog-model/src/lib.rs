//! Small, transport-independent catalog values used by clients and servers.

mod author;
mod canonical;
mod search;

pub use author::{AuthorName, AuthorNameError, AuthorNameMatchKey};
pub use canonical::{canonical_doi, canonical_isbn, canonical_uuid};
pub use search::{normalize_search_text, normalized_substring_match};
