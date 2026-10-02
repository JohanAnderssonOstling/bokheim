//! Portable enrichment policy: the metadata server runs it; clients use the
//! file-evidence parts at import. Storage and transport belong to callers.
pub mod evidence;
pub mod filename_identity;
pub mod filename_isbn;
pub mod related_isbn;
pub mod title_identity;
use book_model::{from_metadata_value, BookMetadata};

/// A subject search is finished once an LCC code resolves in the taxonomy.
/// BISAC and unresolvable LCC codes never end it.
pub fn has_resolved_lcc(metadata: &BookMetadata) -> bool {
    metadata.subjects.iter().any(|s| s.authority().is_some_and(|a| a.eq_ignore_ascii_case("lcc")) && s.code().is_some_and(|c| !subject_projection::unified_lcc_subject_paths(c).is_empty()))
}

/// Every ISBN as its ISBN-13, so both forms of one edition compare equal.
pub fn isbn_key(value: &str) -> Option<String> {
    let isbn = from_metadata_value(value)?;
    if isbn.len() == 13 {
        return Some(isbn);
    }
    let mut isbn13 = format!("978{}", &isbn[..9]);
    let sum: u32 = isbn13.bytes().enumerate().map(|(i, c)| u32::from(c - b'0') * if i % 2 == 0 { 1 } else { 3 }).sum();
    isbn13.push(char::from(b'0' + ((10 - sum % 10) % 10) as u8));
    Some(isbn13)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_resolvable_lcc_ends_the_subject_search() {
        let subject = |authority: &str, code: &str| book_model::BookSubject::new(None, code, "test", Some(authority.into()), Some(code.into())).unwrap();
        let mut metadata = BookMetadata::default();
        metadata.subjects.push(subject("bisac", "COM051010"));
        metadata.subjects.push(subject("lcc", "NOT-A-CODE"));
        assert!(!has_resolved_lcc(&metadata));
        metadata.subjects.push(subject("lcc", "Q125"));
        assert!(has_resolved_lcc(&metadata));
    }

    #[test]
    fn isbn_10_and_13_share_a_key() {
        assert_eq!(isbn_key("0-201-63361-2").as_deref(), Some("9780201633610"));
        assert_eq!(isbn_key("9780201633610").as_deref(), Some("9780201633610"));
    }

    #[test]
    fn whole_values_and_explicit_wrappers() {
        for value in ["978-1-934356-55-5", "978 1 934356 55 5", "urn:isbn:9781934356555", "URN|ISBN|9781934356555"] {
            assert_eq!(from_metadata_value(value).as_deref(), Some("9781934356555"));
        }
        assert_eq!(from_metadata_value("urn:uuid:isbn 0-330-26272-6").as_deref(), Some("0330262726"));
    }
    #[test]
    fn survey_false_positives_are_rejected() {
        for value in [
            "sh2008111148",
            "sh2008106306",
            "7872d3e9-8394-4530-8a39-773505e19c9b",
            "{606DBDD5-6984-4832-9CC6-A289A9696E70}",
            "frus1977-80v27-2025-08-21T21:06:50.413Z",
            "x9780199588503_450.jpg",
            "0000000000000",
            "0000000000",
            "9781934356556",
            "https://example.org/9781934356555",
            "urn:9781934356555",
        ] {
            assert_eq!(from_metadata_value(value), None, "{value}");
        }
    }
}

pub mod resolution;

mod filename_identifiers;

pub mod filename_lookup;
