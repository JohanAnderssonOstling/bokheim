//! Browse text stored on the book row. Rust normalizes it when titles and
//! credits are written; queries only compare and sort.
use book_model::normalize_search_text;

/// Normalized `title: subtitle`, used for both search and alphabetical order.
/// An untitled book is found and sorted by the name it is shown under.
pub(crate) fn search_title(content_hash: &str, title: Option<&str>, subtitle: Option<&str>) -> String {
    let title = title.map_or_else(|| format!("Untitled ({content_hash})"), str::to_owned);
    match subtitle {
        Some(subtitle) => normalize_search_text(&format!("{title}: {subtitle}")),
        None => normalize_search_text(&title),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn title_and_subtitle_are_searched_as_one_normalized_string() {
        let stored = super::search_title("h", Some("École"), Some("Über les livres"));
        assert!(stored.contains(&super::normalize_search_text("  ECOLE:   uber ")));
        assert!(!stored.contains(&super::normalize_search_text("ecole uber")), "punctuation remains significant");
        assert_eq!(super::search_title("h", None, None), super::normalize_search_text("Untitled (h)"));
    }
}
