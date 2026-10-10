//! Browse text stored on the book row. Rust normalizes it when titles and
//! credits are written; queries only compare and sort.
use book_model::normalize_search_text;

/// Normalized `title: subtitle`, used for both search and alphabetical order.
/// An untitled book is found and sorted by the name it is shown under.
pub(crate) fn display_title(title: Option<&str>) -> &str {
    title.filter(|title| !title.trim().is_empty()).unwrap_or("Untitled")
}

pub(crate) fn search_title(title: Option<&str>, subtitle: Option<&str>) -> String {
    let title = display_title(title);
    match subtitle {
        Some(subtitle) => normalize_search_text(&format!("{title}: {subtitle}")),
        None => normalize_search_text(title),
    }
}
