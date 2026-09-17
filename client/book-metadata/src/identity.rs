//! Shared bibliographic lookup preparation and checksum-valid filename ISBNs.
pub use metadata_contract::identity_evidence::{edition_identity_evidence, EditionIdentityEvidence};
use std::collections::BTreeSet;

/// Returns one unambiguous, checksum-valid ISBN embedded in a filename-like
/// title. Multiple distinct ISBNs deliberately return `None`.
pub fn isbn_from_filename_like_title(title: &str) -> Option<String> {
    let mut candidate = String::new();
    let mut isbns = BTreeSet::new();
    let finish_candidate = |candidate: &mut String, isbns: &mut BTreeSet<String>| {
        if let Some(isbn) = book_model::canonical_isbn(candidate) {
            isbns.insert(isbn);
        }
        candidate.clear();
    };
    for character in title.chars() {
        if character.is_ascii_digit() || matches!(character, 'x' | 'X' | '-') {
            candidate.push(character);
        } else if !candidate.is_empty() {
            finish_candidate(&mut candidate, &mut isbns);
        }
    }
    if !candidate.is_empty() {
        finish_candidate(&mut candidate, &mut isbns);
    }
    (isbns.len() == 1).then(|| isbns.into_iter().next()).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_explicit_filename_author() {
        assert_eq!(edition_identity_evidence("An Illustrated Guide (by-Kenneth-Ray-Stubbs)"), EditionIdentityEvidence { title: "An Illustrated Guide".to_owned(), author: Some("Kenneth Ray Stubbs".to_owned()), book_year: None });
    }

    #[test]
    fn extracts_plain_person_parenthesis_conservatively() {
        assert_eq!(edition_identity_evidence("An Illustrated Guide (Kenneth Ray Stubbs)").author.as_deref(), Some("Kenneth Ray Stubbs"));
        assert_eq!(edition_identity_evidence("Physical Chemistry (Second Edition)").author, None);
    }

    #[test]
    fn extracts_noisy_filename_title_author_and_year() {
        assert_eq!(
            edition_identity_evidence("(it-ebooks-2019) it-ebooks - Algorithms (Jeff Erickson)-iBooker it-ebooks (2019)(1)"),
            EditionIdentityEvidence { title: "Algorithms".to_owned(), author: Some("Jeff Erickson".to_owned()), book_year: Some(2019) }
        );
    }

    #[test]
    fn extracts_inline_by_author_before_source_tag() {
        assert_eq!(
            edition_identity_evidence("A Concise History of France by Roger Price (z-lib.org)"),
            EditionIdentityEvidence { title: "A Concise History of France".to_owned(), author: Some("Roger Price".to_owned()), book_year: None }
        );
    }

    #[test]
    fn filename_isbn_requires_one_checksum_valid_value() {
        assert_eq!(isbn_from_filename_like_title("9780198571001.pdf").as_deref(), Some("9780198571001"));
        assert_eq!(isbn_from_filename_like_title("book-978-0-19-857100-1 scan.pdf").as_deref(), Some("9780198571001"));
        assert_eq!(isbn_from_filename_like_title("9780198571002.pdf"), None);
        assert_eq!(isbn_from_filename_like_title("9780198571001 and 9781501168697.pdf"), None);
    }

    #[test]
    fn lookup_cleanup_preserves_qualifiers_and_removes_archive_debris() {
        let evidence = edition_identity_evidence("(by-Anne-Hooper)-Great-Sex-Guide-692141-(z-lib.org)");
        assert_eq!(evidence.title, "Great-Sex-Guide");
        assert_eq!(evidence.author.as_deref(), Some("Anne Hooper"));
        let evidence = edition_identity_evidence("A Concise History of Switzerland (Cambridge Concise Histories)");
        assert_eq!(evidence.title, "A Concise History of Switzerland");
        assert!(evidence.author.is_none());
        assert_eq!(edition_identity_evidence("History (Volume 2)").title, "History (Volume 2)");
        assert_eq!(edition_identity_evidence("History (Revised Edition)").title, "History (Revised Edition)");
    }
}
