use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::canonical::{canonical_doi, canonical_isbn, canonical_uuid};
use crate::validation::bounded;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scheme {
    Unspecified,
    Isbn,
    Asin,
    Doi,
    Issn,
    Oclc,
    Lccn,
    Uuid,
    Calibre,
    Google,
    Goodreads,
    Kobo,
    Other(String),
}

impl Scheme {
    pub fn from_label(value: Option<&str>) -> Self {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else { return Self::Unspecified };
        let normalized = value.to_ascii_lowercase();
        match normalized.as_str() {
            "isbn" | "urn:isbn" => Self::Isbn,
            "asin" => Self::Asin,
            "doi" | "https://doi.org" => Self::Doi,
            "issn" => Self::Issn,
            "oclc" => Self::Oclc,
            "lccn" => Self::Lccn,
            "uuid" | "urn:uuid" => Self::Uuid,
            "calibre" => Self::Calibre,
            "google" | "goog" => Self::Google,
            "goodreads" => Self::Goodreads,
            "kobo" => Self::Kobo,
            _ => Self::Other(value.to_owned()),
        }
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Unspecified => None,
            Self::Isbn => Some("isbn"),
            Self::Asin => Some("asin"),
            Self::Doi => Some("doi"),
            Self::Issn => Some("issn"),
            Self::Oclc => Some("oclc"),
            Self::Lccn => Some("lccn"),
            Self::Uuid => Some("uuid"),
            Self::Calibre => Some("calibre"),
            Self::Google => Some("google"),
            Self::Goodreads => Some("goodreads"),
            Self::Kobo => Some("kobo"),
            Self::Other(value) => Some(value),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Book,
    Edition,
    Work,
}

/// One bibliographic identifier. Source-entry keys are deliberately not
/// represented here; they belong to the source listing identity.
/// Identity carries no provenance: a checksum-valid identifier is the same
/// identifier wherever it was observed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Identifier {
    pub value: String,
    pub scheme: Scheme,
    pub scope: Scope,
}

impl Identifier {
    pub fn new(value: impl AsRef<str>, scheme: Scheme, scope: Scope) -> Result<Self, IdentifierError> {
        Ok(Self { value: bounded(value.as_ref()).map_err(|_| IdentifierError)?, scheme, scope })
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn scheme(&self) -> &Scheme {
        &self.scheme
    }

    pub const fn scope(&self) -> Scope {
        self.scope
    }

    pub fn canonical_value(&self) -> Option<String> {
        match self.scheme {
            Scheme::Isbn => canonical_isbn(&self.value),
            Scheme::Doi => canonical_doi(&self.value),
            Scheme::Uuid => canonical_uuid(&self.value),
            _ => None,
        }
    }

    pub fn lookup_value(&self) -> String {
        self.canonical_value().unwrap_or_else(|| self.value.clone())
    }
}

impl<'de> Deserialize<'de> for Identifier {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            value: String,
            scheme: Scheme,
            scope: Scope,
        }
        let encoded = Encoded::deserialize(deserializer)?;
        Self::new(encoded.value, encoded.scheme, encoded.scope).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentifierError;

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("identifier value or source is invalid")
    }
}

impl Error for IdentifierError {}

/// Accept a complete ISBN value, optionally wrapped in an explicit ISBN label.
/// Never carve numbers out of identifiers, URLs, filenames or dates.
pub fn from_metadata_value(value: &str) -> Option<String> {
    fn whole(value: &str) -> Option<String> {
        let compact: String = value.chars().filter(|c| !c.is_whitespace() && !matches!(c, '-' | '\u{00ad}' | '\u{2010}'..='\u{2015}')).collect();
        if !(compact.len() == 10 || (compact.len() == 13 && (compact.starts_with("978") || compact.starts_with("979")))) || compact.chars().all(|c| c == '0') {
            return None;
        }
        if !compact.chars().enumerate().all(|(i, c)| c.is_ascii_digit() || (compact.len() == 10 && i == 9 && matches!(c, 'X' | 'x'))) {
            return None;
        }
        canonical_isbn(&compact)
    }
    let value = value.trim();
    if let Some(isbn) = whole(value) {
        return Some(isbn);
    }
    let mut tail = value;
    // Preserve the previously supported malformed urn:uuid:isbn wrapper, but
    // require an explicit ISBN label; plain UUIDs are never normalized.
    for prefix in ["urn", "uuid"] {
        if tail.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix)) {
            let rest = &tail[prefix.len()..];
            let rest = rest.strip_prefix(':').or_else(|| rest.strip_prefix('|'))?;
            tail = rest.trim_start();
        }
    }
    if !tail.get(..4).is_some_and(|head| head.eq_ignore_ascii_case("isbn")) {
        return None;
    }
    let rest = &tail[4..];
    if !rest.starts_with(|c: char| c == ':' || c == '|' || c.is_whitespace()) {
        return None;
    }
    whole(rest.trim_start_matches(|c: char| c == ':' || c == '|' || c.is_whitespace()))
}

/// Add a canonical ISBN only when no ISBN with the same canonical value exists.
/// The caller retains ownership of the source-specific scope.
pub fn push_isbn(identifiers: &mut Vec<Identifier>, isbn: impl AsRef<str>, scope: Scope) -> Result<(), IdentifierError> {
    let isbn = canonical_isbn(isbn.as_ref()).unwrap_or_else(|| isbn.as_ref().to_owned());
    if identifiers.iter().any(|identifier| identifier.scheme() == &Scheme::Isbn && identifier.canonical_value().as_deref() == Some(isbn.as_str())) {
        return Ok(());
    }
    identifiers.push(Identifier::new(isbn, Scheme::Isbn, scope)?);
    Ok(())
}
