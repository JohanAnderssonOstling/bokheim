//! Stored navigation documents: reads, replacement, and enrichment overlays.

use include_sqlite_sql::include_sql;

use library_model::BookTocEntry;

include_sql!("src/books/sql/toc.sql");

/// The title of one entry in a stored tree. The target is the identity; the
/// wording comes from whatever navigation document is stored now.
pub fn entry_title(entries: &[BookTocEntry], target: &str) -> Option<String> {
    for entry in entries {
        if entry.target == target {
            return Some(entry.title.clone());
        }
        if let Some(found) = entry_title(&entry.children, target) {
            return Some(found);
        }
    }
    None
}
