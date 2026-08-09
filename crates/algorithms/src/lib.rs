//! Pure, platform-independent algorithms shared by native, server, and web clients.

use std::fmt;

pub mod publication;
pub use publication::{
    Contributor, LanguageTag, PublicationCollection, PublicationDate, PublicationIdentifier, PublicationIdentifierScheme, PublicationIdentifierType, PublicationMetadata, PublicationMetadataError, PublicationProperty,
    PublicationSubject, PublicationTitle, PublisherCredit, PublisherMatchKey, PublisherName,
};

/// Opaque MARC relator code carried by persistence and synchronization.
///
/// Sync and storage only ever move these three bytes around; interpreting
/// what a code means (author vs. translator vs. ...) is a client concern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct MarcRelatorCode(pub [u8; 3]);

/// A canonical reading location shared by all reader formats.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReadingPosition {
    Epub(EpubCfi),
    Pdf(u32),
    Audiobook(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EpubCfi(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidReadingPosition;

impl ReadingPosition {
    pub fn parse(value: &str) -> Result<Self, InvalidReadingPosition> {
        if valid_epub_cfi(value) {
            return Ok(Self::Epub(EpubCfi(value.to_owned())));
        }
        if let Some(page) = pdf_page_from_reading_position(value) {
            return Ok(Self::Pdf(page));
        }
        if let Some(millis) = audiobook_position_millis(value) {
            return Ok(Self::Audiobook(millis));
        }
        Err(InvalidReadingPosition)
    }

    pub fn epub_cfi(value: &str) -> Result<Self, InvalidReadingPosition> {
        match Self::parse(value)? {
            Self::Epub(cfi) => Ok(Self::Epub(cfi)),
            _ => Err(InvalidReadingPosition),
        }
    }

    pub const fn pdf_page(page_index: u32) -> Self {
        Self::Pdf(page_index)
    }
    pub const fn audiobook_millis(millis: u64) -> Self {
        Self::Audiobook(millis)
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::Epub(cfi) => cfi.0.clone(),
            Self::Pdf(page) => pdf_page_reading_position(*page),
            Self::Audiobook(millis) => audiobook_reading_position(*millis),
        }
    }

    pub fn epub_cfi_value(&self) -> Option<&str> {
        match self {
            Self::Epub(cfi) => Some(&cfi.0),
            _ => None,
        }
    }

    pub fn pdf_page_index(&self) -> Option<u32> {
        match self {
            Self::Pdf(page) => Some(*page),
            _ => None,
        }
    }

    pub fn audiobook_position_millis(&self) -> Option<u64> {
        match self {
            Self::Audiobook(millis) => Some(*millis),
            _ => None,
        }
    }
}

impl fmt::Display for InvalidReadingPosition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid canonical reading position")
    }
}

impl std::error::Error for InvalidReadingPosition {}

impl serde::Serialize for ReadingPosition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for ReadingPosition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// Validates the protocol-level EPUB CFI representation. Only the shape
/// (`epubcfi(...)`), a size bound, and the absence of control characters are
/// enforced here; the CFI grammar itself is the reading client's concern, not
/// something the sync protocol re-parses on every write.
pub fn valid_epub_cfi(value: &str) -> bool {
    if value.len() > 8 * 1024 || value.trim() != value || value.chars().any(char::is_control) {
        return false;
    }
    let Some(inner) = value.strip_prefix("epubcfi(").and_then(|value| value.strip_suffix(')')) else {
        return false;
    };
    !inner.is_empty()
}

/// Validates the synchronized position representation for EPUB, PDF, and audiobook readers.
pub fn valid_reading_position(value: &str) -> bool {
    ReadingPosition::parse(value).is_ok()
}

/// Extracts the zero-based page index from a canonical PDF reading position.
pub fn pdf_page_from_reading_position(value: &str) -> Option<u32> {
    let page = value.strip_prefix("pdfpage(")?.strip_suffix(')')?;
    if page.is_empty() || (page.len() > 1 && page.starts_with('0')) {
        return None;
    }
    page.parse().ok()
}

/// Creates the canonical synchronized PDF reading position.
pub fn pdf_page_reading_position(page_index: u32) -> String {
    format!("pdfpage({page_index})")
}

/// Extracts the playback timestamp from a canonical audiobook reading position.
pub fn audiobook_position_millis(value: &str) -> Option<u64> {
    let millis = value.strip_prefix("audiopos(")?.strip_suffix(')')?;
    if millis.is_empty() || (millis.len() > 1 && millis.starts_with('0')) {
        return None;
    }
    millis.parse().ok()
}

/// Creates the canonical synchronized audiobook reading position.
pub fn audiobook_reading_position(millis: u64) -> String {
    format!("audiopos({millis})")
}

/// Normalizes an ISBN-10 or ISBN-13 to a compact, checksum-verified form.
/// Returns `None` if the value's check digit does not validate, since a
/// canonical form is only meaningful for an ISBN that is actually valid.
pub fn canonical_isbn(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.strip_prefix("urn:isbn:").or_else(|| value.strip_prefix("URN:ISBN:")).unwrap_or(value);
    let compact = value.chars().filter(|character| !matches!(character, '-' | ' ')).map(|character| character.to_ascii_uppercase()).collect::<String>();
    let bytes = compact.as_bytes();
    let valid = match bytes.len() {
        10 => {
            bytes[..9].iter().all(u8::is_ascii_digit)
                && (bytes[9].is_ascii_digit() || bytes[9] == b'X')
                && bytes.iter().enumerate().map(|(index, byte)| u32::from(if *byte == b'X' { 10 } else { *byte - b'0' }) * (10 - index as u32)).sum::<u32>() % 11 == 0
        }
        13 => bytes.iter().all(u8::is_ascii_digit) && bytes.iter().enumerate().map(|(index, byte)| u32::from(*byte - b'0') * if index % 2 == 0 { 1 } else { 3 }).sum::<u32>() % 10 == 0,
        _ => false,
    };
    valid.then_some(compact)
}

/// Normalizes a DOI, stripping resolver URL prefixes, to its bare
/// `10.xxxx/suffix` form. Returns `None` if the value does not have that shape.
pub fn canonical_doi(value: &str) -> Option<String> {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    let value = lower
        .strip_prefix("https://doi.org/")
        .or_else(|| lower.strip_prefix("http://doi.org/"))
        .or_else(|| lower.strip_prefix("https://dx.doi.org/"))
        .or_else(|| lower.strip_prefix("http://dx.doi.org/"))
        .or_else(|| lower.strip_prefix("doi:"))
        .unwrap_or(&lower);
    let valid = value.starts_with("10.")
        && value.split_once('/').is_some_and(|(registrant, suffix)| (4..=9).contains(&registrant[3..].len()) && registrant[3..].bytes().all(|byte| byte.is_ascii_digit()) && !suffix.is_empty())
        && !value.bytes().any(|byte| byte.is_ascii_whitespace());
    valid.then_some(value.to_owned())
}

/// Normalizes a UUID (accepting an optional `urn:uuid:` prefix) to its
/// canonical lowercase hyphenated form.
pub fn canonical_uuid(value: &str) -> Option<String> {
    let lower = value.trim().to_ascii_lowercase();
    let value = lower.strip_prefix("urn:uuid:").unwrap_or(&lower);
    uuid::Uuid::parse_str(value).ok().map(|value| value.hyphenated().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_isbn_only_accepts_checksum_valid_values() {
        assert_eq!(canonical_isbn("urn:isbn:978-1-23456-789-7"), Some("9781234567897".to_owned()));
        assert_eq!(canonical_isbn("978-1-23456-789-8"), None);
    }

    #[test]
    fn canonical_doi_strips_resolver_prefixes() {
        assert_eq!(canonical_doi("HTTPS://DOI.ORG/10.1000/Example"), Some("10.1000/example".to_owned()));
        assert_eq!(canonical_doi("10./not-a-doi"), None);
    }

    #[test]
    fn canonical_uuid_normalizes_case_and_urn_prefix() {
        assert_eq!(canonical_uuid("URN:UUID:00112233-4455-6677-8899-AABBCCDDEEFF"), Some("00112233-4455-6677-8899-aabbccddeeff".to_owned()));
        assert_eq!(canonical_uuid("not-a-uuid"), None);
    }

    #[test]
    fn validates_epub_positions() {
        assert!(valid_epub_cfi("epubcfi(/6/4[chapter]!/4/2/1:0)"));
        assert!(valid_epub_cfi("epubcfi(/6/4[id^]with^,escapes])"));
        assert!(!valid_epub_cfi("/6/4"));
        assert!(!valid_epub_cfi("epubcfi()"));
        // The CFI grammar itself (bracket/escape structure) is trusted from the
        // client and no longer re-validated here — only shape and size are.
        assert!(valid_epub_cfi("epubcfi(/6/4[unclosed)"));
    }

    #[test]
    fn validates_canonical_pdf_positions() {
        assert_eq!(pdf_page_from_reading_position("pdfpage(0)"), Some(0));
        assert_eq!(pdf_page_from_reading_position("pdfpage(42)"), Some(42));
        assert_eq!(pdf_page_from_reading_position("pdfpage(01)"), None);
        assert_eq!(pdf_page_reading_position(u32::MAX), "pdfpage(4294967295)");
    }

    #[test]
    fn validates_canonical_audiobook_positions() {
        assert_eq!(audiobook_position_millis("audiopos(0)"), Some(0));
        assert_eq!(audiobook_position_millis("audiopos(123456)"), Some(123_456));
        assert_eq!(audiobook_position_millis("audiopos(01)"), None);
        assert_eq!(audiobook_position_millis("audiopos(-1)"), None);
        assert_eq!(audiobook_reading_position(u64::MAX), "audiopos(18446744073709551615)");
        assert!(valid_reading_position("audiopos(123456)"));
    }

    #[test]
    fn typed_positions_round_trip_through_canonical_strings() {
        for value in ["epubcfi(/6/4)", "pdfpage(17)", "audiopos(1234567)"] {
            let position = ReadingPosition::parse(value).unwrap();
            assert_eq!(position.as_str(), value);
            let encoded = serde_json::to_string(&position).unwrap();
            let decoded: ReadingPosition = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, position);
        }
        assert!(ReadingPosition::parse("pdfpage(01)").is_err());
        assert!(ReadingPosition::parse("audiopos(-1)").is_err());
    }
}
