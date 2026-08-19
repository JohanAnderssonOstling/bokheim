use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EpubCfi(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidEpubCfi;

impl EpubCfi {
    pub fn parse(value: &str) -> Result<Self, InvalidEpubCfi> {
        if !valid_cfi(value) {
            return Err(InvalidEpubCfi);
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

fn valid_cfi(value: &str) -> bool {
    if !valid_epub_cfi_shape(value) {
        return false;
    }
    let mut depth = 0_i32;
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '^' {
            escaped = true;
            continue;
        }
        match character {
            '[' => depth += 1,
            ']' if depth == 0 => return false,
            ']' => depth -= 1,
            _ => {}
        }
    }
    depth == 0 && !escaped
}

fn valid_epub_cfi_shape(value: &str) -> bool {
    if value.len() > 8 * 1024 || value.trim() != value || value.chars().any(char::is_control) {
        return false;
    }
    value.strip_prefix("epubcfi(").and_then(|value| value.strip_suffix(')')).is_some_and(|inner| !inner.is_empty())
}

/// A canonical reading location shared by all reader formats.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReadingPosition {
    Epub(ReadingEpubCfi),
    Pdf(u32),
    Audiobook(u64),
}

/// Protocol-level EPUB position. Unlike an annotation anchor, synchronized
/// reading progress validates only bounded CFI shape and leaves full grammar
/// interpretation to the reader implementation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReadingEpubCfi(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidReadingPosition;

impl ReadingPosition {
    pub fn parse(value: &str) -> Result<Self, InvalidReadingPosition> {
        if valid_epub_cfi(value) {
            return Ok(Self::Epub(ReadingEpubCfi(value.to_owned())));
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
        if valid_epub_cfi(value) { Ok(Self::Epub(ReadingEpubCfi(value.to_owned()))) } else { Err(InvalidReadingPosition) }
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

pub fn valid_epub_cfi(value: &str) -> bool {
    valid_epub_cfi_shape(value)
}

pub fn valid_reading_position(value: &str) -> bool {
    ReadingPosition::parse(value).is_ok()
}

pub fn pdf_page_from_reading_position(value: &str) -> Option<u32> {
    let page = value.strip_prefix("pdfpage(")?.strip_suffix(')')?;
    if page.is_empty() || (page.len() > 1 && page.starts_with('0')) {
        return None;
    }
    page.parse().ok()
}

pub fn pdf_page_reading_position(page_index: u32) -> String {
    format!("pdfpage({page_index})")
}

pub fn audiobook_position_millis(value: &str) -> Option<u64> {
    let millis = value.strip_prefix("audiopos(")?.strip_suffix(')')?;
    if millis.is_empty() || (millis.len() > 1 && millis.starts_with('0')) {
        return None;
    }
    millis.parse().ok()
}

pub fn audiobook_reading_position(millis: u64) -> String {
    format!("audiopos({millis})")
}

impl fmt::Display for EpubCfi {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl fmt::Display for InvalidEpubCfi {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("reading position must be a bounded, well-formed EPUB CFI")
    }
}

impl std::error::Error for InvalidEpubCfi {}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct ReadProgress(f32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidReadProgress;

impl ReadProgress {
    pub const ZERO: Self = Self(0.0);

    pub fn new(value: f32) -> Result<Self, InvalidReadProgress> {
        if value.is_finite() && (0.0..=100.0).contains(&value) { Ok(Self(value)) } else { Err(InvalidReadProgress) }
    }

    pub fn value(self) -> f32 {
        self.0
    }
}

impl serde::Serialize for ReadProgress {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f32(self.0)
    }
}

impl<'de> serde::Deserialize<'de> for ReadProgress {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <f32 as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for InvalidReadProgress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("reading progress must be finite and between 0 and 100")
    }
}

impl std::error::Error for InvalidReadProgress {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epub_cfi_rejects_legacy_numbers_and_malformed_delimiters() {
        assert_eq!(EpubCfi::parse("epubcfi(/6/4[id]!/4/2/1:25)").unwrap().as_str(), "epubcfi(/6/4[id]!/4/2/1:25)");
        assert!(EpubCfi::parse("0").is_err());
        assert!(EpubCfi::parse("epubcfi()").is_err());
        assert!(EpubCfi::parse("epubcfi(/6/4[open)").is_err());
        assert!(EpubCfi::parse(" epubcfi(/6/4)").is_err());
    }

    #[test]
    fn read_progress_rejects_non_finite_and_out_of_range_values() {
        assert_eq!(ReadProgress::new(37.5).unwrap().value(), 37.5);
        assert!(ReadProgress::new(f32::NAN).is_err());
        assert!(ReadProgress::new(-0.1).is_err());
        assert!(ReadProgress::new(100.1).is_err());
    }

    #[test]
    fn canonical_positions_round_trip_for_every_reader_format() {
        for value in ["epubcfi(/6/4)", "pdfpage(17)", "audiopos(1234567)"] {
            let position = ReadingPosition::parse(value).unwrap();
            assert_eq!(position.as_str(), value);
            let encoded = serde_json::to_string(&position).unwrap();
            let decoded: ReadingPosition = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, position);
        }
        assert!(ReadingPosition::parse("pdfpage(01)").is_err());
        assert!(ReadingPosition::parse("audiopos(-1)").is_err());
        assert!(ReadingPosition::parse("epubcfi(/6/4[unclosed)").is_ok());
        assert!(EpubCfi::parse("epubcfi(/6/4[unclosed)").is_err());
    }
}
