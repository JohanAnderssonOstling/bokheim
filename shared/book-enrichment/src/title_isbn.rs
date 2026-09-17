//! Checksum-verified ISBN evidence explicitly present in title text.
use book_model::{canonical_isbn, Identifier, BookMetadata, Scheme, Scope, IdentifierError};
use std::collections::BTreeSet;

pub fn extract(title: &str) -> Vec<String> {
    let mut found = BTreeSet::new();
    // Keep whole numeric runs: never accept a valid substring of a longer ID.
    for run in title.split(|c: char| !(c.is_ascii_digit() || matches!(c, 'x' | 'X' | '-' | ' '))) {
        let words = run.split_whitespace().collect::<Vec<_>>();
        for start in 0..words.len() {
            let mut value = String::new();
            for word in &words[start..] {
                value.push_str(word);
                let digits = value.chars().filter(|c| *c != '-').count();
                if digits > 13 { break; }
                if let Some(isbn) = canonical_isbn(&value) {
                    if isbn.len() == 10 || isbn.starts_with("978") || isbn.starts_with("979") {
                        found.insert(isbn);
                    }
                }
            }
        }
    }
    found.into_iter().collect()
}

pub fn record(title: &str, book: &mut BookMetadata, source: &str) -> Result<(), IdentifierError> {
    for isbn in extract(title) {
        if !book.identifiers.iter().any(|id| id.scheme() == &Scheme::Isbn && id.canonical_value().as_deref() == Some(&isbn)) {
            book.identifiers.push(Identifier::new(isbn, Scheme::Isbn, Scope::Edition, source)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_valid_isbns_from_titles_and_filename_wrappers() {
        for title in ["0192805045.pdf", "Fossils (ISBN: 0-19-280504-5)", "Fossils ISBN 0 19 280504 5"] {
            assert_eq!(extract(title), ["0192805045"]);
        }
        assert_eq!(extract("Dreaming ISBN: 978-0-19-280215-6.pdf"), ["9780192802156"]);
        assert!(extract("9780192802157.pdf 1984 1239780192802156123").is_empty());
        assert_eq!(extract("0192805045 and 9780192802156"), ["0192805045", "9780192802156"]);
    }
    #[test]
    fn recording_is_idempotent_and_does_not_infer_an_ebook_identifier() {
        let mut book = BookMetadata::default();
        record("0192805045.pdf", &mut book, "ingestion:title").unwrap();
        let before = book.clone();
        record("ISBN 0-19-280504-5", &mut book, "ingestion:title").unwrap();
        assert_eq!(book, before);
        assert_eq!(book.identifiers[0].scope(), Scope::Edition);
    }
}
