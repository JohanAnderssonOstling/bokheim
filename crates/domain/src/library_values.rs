use serde::{Serialize, Serializer};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct BookTitle(String);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct AuthorName(String);

/// Stable, locale-independent key used to group spelling variants of an
/// author name. It is deliberately not a real-person identity: namesakes have
/// the same key until an author-identity layer can distinguish them using
/// stronger evidence.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AuthorNameMatchKey(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct NonNegativeTimestamp(i64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderPath {
    Root,
    Relative(crate::RelativeDirPath),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LibraryValueError(&'static str);

impl std::fmt::Display for LibraryValueError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for LibraryValueError {}

fn validated_display_text(value: &str, kind: &'static str) -> Result<String, LibraryValueError> {
    let value = value.trim();
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        Err(LibraryValueError(kind))
    } else {
        Ok(value.to_owned())
    }
}

impl BookTitle {
    pub fn parse(value: &str) -> Result<Self, LibraryValueError> {
        validated_display_text(value, "book title must be non-empty, bounded, and contain no control characters").map(Self)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AuthorName {
    pub fn parse(value: &str) -> Result<Self, LibraryValueError> {
        validated_display_text(value, "author name must be non-empty, bounded, and contain no control characters").map(Self)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn match_key(&self) -> AuthorNameMatchKey {
        AuthorNameMatchKey(normalized_author_match_key(&self.0))
    }
}

impl AuthorNameMatchKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn normalized_author_match_key(value: &str) -> String {
    let normalized = value.nfkc().collect::<String>();
    let reordered = comma_reordered_name(&normalized);
    let reordered = reordered.as_deref().unwrap_or(normalized.as_str());
    let mut key = String::with_capacity(reordered.len());
    let mut separator_pending = false;
    for character in reordered.chars() {
        if character.is_alphanumeric() || is_combining_mark(character) {
            if separator_pending && !key.is_empty() {
                key.push(' ');
            }
            separator_pending = false;
            key.extend(character.to_lowercase());
        } else {
            separator_pending = true;
        }
    }
    key
}

fn comma_reordered_name(value: &str) -> Option<String> {
    if looks_like_organization(value) {
        return None;
    }
    let parts = value.split(',').map(str::trim).collect::<Vec<_>>();
    match parts.as_slice() {
        [family, given] if !family.is_empty() && !given.is_empty() => Some(format!("{given} {family}")),
        [family, given, suffix] if !family.is_empty() && !given.is_empty() && recognized_suffix(suffix) => Some(format!("{given} {family} {suffix}")),
        _ => None,
    }
}

fn recognized_suffix(value: &str) -> bool {
    matches!(normalized_author_match_key_without_reordering(value).as_str(), "jr" | "sr" | "ii" | "iii" | "iv")
}

fn normalized_author_match_key_without_reordering(value: &str) -> String {
    let mut key = String::new();
    let mut separator_pending = false;
    for character in value.nfkc() {
        if character.is_alphanumeric() || is_combining_mark(character) {
            if separator_pending && !key.is_empty() {
                key.push(' ');
            }
            separator_pending = false;
            key.extend(character.to_lowercase());
        } else {
            separator_pending = true;
        }
    }
    key
}

fn looks_like_organization(value: &str) -> bool {
    if value.contains('&') {
        return true;
    }
    let key = normalized_author_match_key_without_reordering(value);
    key.split_whitespace().any(|word| matches!(word, "association" | "committee" | "company" | "corporation" | "institute" | "press" | "society" | "university" | "inc" | "llc" | "ltd"))
}

impl NonNegativeTimestamp {
    pub const ZERO: Self = Self(0);

    pub fn new(value: i64) -> Result<Self, LibraryValueError> {
        if value >= 0 {
            Ok(Self(value))
        } else {
            Err(LibraryValueError("timestamp must not be negative"))
        }
    }
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl FolderPath {
    pub fn parse(value: &str) -> Result<Self, LibraryValueError> {
        if value == "(Root)" || value.is_empty() {
            return Ok(Self::Root);
        }
        crate::RelativeDirPath::parse(value).map(Self::Relative).map_err(|_| LibraryValueError("folder path must be root or a valid relative directory path"))
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Root => "(Root)",
            Self::Relative(path) => path.as_str(),
        }
    }
}

impl Serialize for FolderPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: &str) -> String {
        AuthorName::parse(value).unwrap().match_key().as_str().to_owned()
    }

    #[test]
    fn author_match_key_normalizes_initial_punctuation_case_and_spacing() {
        assert_eq!(key(" J.R.R.   Tolkien "), key("j. r. r. tolkien"));
    }

    #[test]
    fn author_match_key_uses_explicit_epub_family_name_order() {
        assert_eq!(key("Tolkien, J. R. R."), key("J.R.R. Tolkien"));
        assert_eq!(key("Le Guin, Ursula K."), key("Ursula K. Le Guin"));
        assert_eq!(key("Smith, John, Jr."), key("John Smith Jr."));
    }

    #[test]
    fn author_match_key_normalizes_equivalent_unicode_without_removing_diacritics() {
        assert_eq!(key("Jose\u{301} Saramago"), key("Jos\u{e9} Saramago"));
        assert_ne!(key("Garc\u{ed}a M\u{e1}rquez"), key("Garcia Marquez"));
    }

    #[test]
    fn author_match_key_does_not_infer_unmarked_or_organizational_name_order() {
        assert_ne!(key("Mao Zedong"), key("Zedong Mao"));
        assert_ne!(key("Smith, Jones & Company"), key("Jones Company Smith"));
        assert_eq!(key("\u{738b}\u{5c0f}\u{660e}"), "\u{738b}\u{5c0f}\u{660e}");
    }
}
