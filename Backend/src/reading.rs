use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpubCfi(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidEpubCfi;

impl EpubCfi {
    pub fn parse(value: &str) -> Result<Self, InvalidEpubCfi> {
        if !sync_common::valid_epub_cfi(value) {
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
        if value.is_finite() && (0.0..=100.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidReadProgress)
        }
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
}
