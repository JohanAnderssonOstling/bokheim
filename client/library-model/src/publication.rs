//! Bibliographic value types: publisher, language, identifier, contributor,
//! and other publication metadata carried by synchronized book state.
//!
//! These types are validated data, not synchronization logic; the merge
//! policy that treats a whole [`PublicationMetadata`] value as a single
//! last-writer-wins register lives with the mutation protocol, not here.

use crate::{MarcRelatorCode, canonical_doi, canonical_isbn, canonical_uuid};
use serde::{Deserialize, Serialize};
use std::fmt;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

pub type AgentId = uuid::Uuid;
pub type AuthorId = AgentId;
pub type PublisherId = AgentId;
pub type ContributorId = AgentId;

pub fn agent_id_from_migration_seed(seed: &[u8]) -> AgentId {
    const NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes([0x7f, 0xe7, 0x7e, 0x34, 0x29, 0x7f, 0x4a, 0xfa, 0x99, 0x8d, 0x2f, 0x86, 0xf0, 0x58, 0x7b, 0x31]);
    uuid::Uuid::new_v5(&NAMESPACE, seed)
}

/// A publisher spelling exactly as credited by a publication. Equality is
/// deliberately distinct from publisher identity; [`PublisherName::match_key`]
/// supplies the conservative grouping key used by library indexes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct PublisherName(String);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PublisherMatchKey(String);

impl PublisherName {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, PublicationMetadataError> {
        normalized_publication_text(value.as_ref()).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn match_key(&self) -> PublisherMatchKey {
        let normalized = self.0.nfkc().collect::<String>();
        let mut key = String::with_capacity(normalized.len());
        let mut separator_pending = false;
        for character in normalized.chars() {
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
        PublisherMatchKey(key)
    }
}

impl PublisherMatchKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PublisherName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// One exact publisher credit linked to a stable Bokheim agent. The credited
/// spelling remains publication evidence; identity resolution can later add
/// authority identifiers without rewriting that evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublisherCredit {
    publisher_id: PublisherId,
    name: PublisherName,
}

impl PublisherCredit {
    #[cfg(feature = "random-ids")]
    pub fn new(name: impl AsRef<str>) -> Result<Self, PublicationMetadataError> {
        Self::with_id(PublisherId::new_v4(), name)
    }

    pub fn with_id(publisher_id: PublisherId, name: impl AsRef<str>) -> Result<Self, PublicationMetadataError> {
        Ok(Self { publisher_id, name: PublisherName::parse(name)? })
    }

    pub const fn publisher_id(&self) -> PublisherId {
        self.publisher_id
    }

    pub const fn agent_id(&self) -> AgentId {
        self.publisher_id
    }

    pub fn name(&self) -> &PublisherName {
        &self.name
    }
}

impl<'de> Deserialize<'de> for PublisherCredit {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct EncodedPublisherCredit {
            publisher_id: PublisherId,
            name: PublisherName,
        }

        let encoded = EncodedPublisherCredit::deserialize(deserializer)?;
        Self::with_id(encoded.publisher_id, encoded.name.as_str()).map_err(serde::de::Error::custom)
    }
}

/// Canonical, case-insensitive representation of an EPUB language tag.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct LanguageTag(String);

impl LanguageTag {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, PublicationMetadataError> {
        let value = value.as_ref().trim();
        let syntactically_valid = !value.is_empty()
            && value.len() <= 63
            && value.split('-').all(|part| !part.is_empty() && part.len() <= 8 && part.chars().all(|character| character.is_ascii_alphanumeric()))
            && value.split('-').next().is_some_and(|part| ((2..=8).contains(&part.len()) || part.eq_ignore_ascii_case("x")) && part.chars().all(|character| character.is_ascii_alphabetic()));
        if !syntactically_valid {
            return Err(PublicationMetadataError);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for LanguageTag {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationIdentifierScheme {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationIdentifierType {
    value: String,
    scheme: Option<String>,
}

impl PublicationIdentifierType {
    pub fn new(value: impl AsRef<str>, scheme: Option<String>) -> Result<Self, PublicationMetadataError> {
        Ok(Self { value: bounded_publication_text(value.as_ref())?, scheme: scheme.map(|value| bounded_publication_text(&value)).transpose()? })
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }
}

impl<'de> Deserialize<'de> for PublicationIdentifierType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct EncodedPublicationIdentifierType {
            value: String,
            #[serde(default)]
            scheme: Option<String>,
        }

        let encoded = EncodedPublicationIdentifierType::deserialize(deserializer)?;
        Self::new(encoded.value, encoded.scheme).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationIdentifier {
    value: String,
    scheme: PublicationIdentifierScheme,
    identifier_types: Vec<PublicationIdentifierType>,
}

impl PublicationIdentifier {
    pub fn new(value: impl AsRef<str>, scheme: PublicationIdentifierScheme, identifier_types: Vec<PublicationIdentifierType>) -> Result<Self, PublicationMetadataError> {
        let value = bounded_publication_text(value.as_ref())?;
        if identifier_types.len() > 32 {
            return Err(PublicationMetadataError);
        }
        Ok(Self { value, scheme, identifier_types })
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    /// Recomputed from `value` and `scheme` on every call rather than stored,
    /// so it can never disagree with the identifier it was derived from.
    pub fn canonical_value(&self) -> Option<String> {
        canonical_identifier_value(&self.scheme, &self.value)
    }

    pub fn lookup_value(&self) -> String {
        self.canonical_value().unwrap_or_else(|| self.value.clone())
    }

    pub fn scheme(&self) -> &PublicationIdentifierScheme {
        &self.scheme
    }

    pub fn identifier_types(&self) -> &[PublicationIdentifierType] {
        &self.identifier_types
    }
}

impl<'de> Deserialize<'de> for PublicationIdentifier {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct EncodedPublicationIdentifier {
            value: String,
            scheme: PublicationIdentifierScheme,
            #[serde(default)]
            identifier_types: Vec<PublicationIdentifierType>,
        }

        let encoded = EncodedPublicationIdentifier::deserialize(deserializer)?;
        Self::new(encoded.value, encoded.scheme, encoded.identifier_types).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Contributor {
    contributor_id: ContributorId,
    name: String,
    sort_name: Option<String>,
    role: MarcRelatorCode,
}

impl Contributor {
    #[cfg(feature = "random-ids")]
    pub fn new(name: impl AsRef<str>, sort_name: Option<String>, role: MarcRelatorCode) -> Result<Self, PublicationMetadataError> {
        Self::with_id(ContributorId::new_v4(), name, sort_name, role)
    }

    pub fn with_id(contributor_id: ContributorId, name: impl AsRef<str>, sort_name: Option<String>, role: MarcRelatorCode) -> Result<Self, PublicationMetadataError> {
        Ok(Self { contributor_id, name: normalized_publication_text(name.as_ref())?, sort_name: sort_name.map(|value| normalized_publication_text(&value)).transpose()?, role })
    }

    pub const fn contributor_id(&self) -> ContributorId {
        self.contributor_id
    }

    pub const fn agent_id(&self) -> AgentId {
        self.contributor_id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn sort_name(&self) -> Option<&str> {
        self.sort_name.as_deref()
    }

    pub const fn role_code(&self) -> MarcRelatorCode {
        self.role
    }
}

impl<'de> Deserialize<'de> for Contributor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct EncodedContributor {
            contributor_id: ContributorId,
            name: String,
            sort_name: Option<String>,
            role: MarcRelatorCode,
        }

        let encoded = EncodedContributor::deserialize(deserializer)?;
        Self::with_id(encoded.contributor_id, encoded.name, encoded.sort_name, encoded.role).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationDate {
    id: Option<String>,
    value: String,
    event: Option<String>,
}

impl PublicationDate {
    pub fn new(id: Option<String>, value: impl AsRef<str>, event: Option<String>) -> Result<Self, PublicationMetadataError> {
        let id = id.map(|value| bounded_publication_text(&value)).transpose()?;
        let value = bounded_publication_text(value.as_ref())?;
        let event = event.map(|value| bounded_publication_text(&value)).transpose()?;
        Ok(Self { id, value, event })
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn event(&self) -> Option<&str> {
        self.event.as_deref()
    }
}

impl<'de> Deserialize<'de> for PublicationDate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct EncodedPublicationDate {
            id: Option<String>,
            value: String,
            event: Option<String>,
        }

        let encoded = EncodedPublicationDate::deserialize(deserializer)?;
        Self::new(encoded.id, encoded.value, encoded.event).map_err(serde::de::Error::custom)
    }
}

/// Returns a publication year only when the supplied date evidence contains
/// exactly one plausible four-digit year.
pub fn unique_publication_year<'a>(values: impl Iterator<Item = &'a str>) -> Option<i32> {
    let mut years = std::collections::BTreeSet::new();
    for value in values {
        for run in value.split(|character: char| !character.is_ascii_digit()) {
            if run.len() == 4 {
                if let Ok(year) = run.parse::<i32>() {
                    if (1000..=2100).contains(&year) {
                        years.insert(year);
                    }
                }
            }
        }
    }
    (years.len() == 1).then(|| *years.first().expect("one publication year exists"))
}

#[derive(Clone, Debug, Serialize)]
pub struct PublicationSubject {
    id: Option<String>,
    name: String,
    // Parser provenance is local indexing evidence, not synchronized
    // bibliographic meaning. Skipping it preserves the versioned wire shape.
    #[serde(skip)]
    source: String,
    authority: Option<String>,
    code: Option<String>,
}

impl PartialEq for PublicationSubject {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.name == other.name && self.authority == other.authority && self.code == other.code
    }
}

impl Eq for PublicationSubject {}

impl PublicationSubject {
    pub fn new(id: Option<String>, name: impl AsRef<str>, source: impl AsRef<str>, authority: Option<String>, code: Option<String>) -> Result<Self, PublicationMetadataError> {
        Ok(Self {
            id: id.map(|value| bounded_publication_text(&value)).transpose()?,
            name: bounded_publication_text(name.as_ref())?,
            source: bounded_publication_text(source.as_ref())?,
            authority: authority.map(|value| bounded_publication_text(&value)).transpose()?,
            code: code.map(|value| bounded_publication_text(&value)).transpose()?,
        })
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn authority(&self) -> Option<&str> {
        self.authority.as_deref()
    }

    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }
}

impl<'de> Deserialize<'de> for PublicationSubject {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            id: Option<String>,
            name: String,
            authority: Option<String>,
            code: Option<String>,
        }
        let encoded = Encoded::deserialize(deserializer)?;
        Self::new(encoded.id, encoded.name, default_subject_source(), encoded.authority, encoded.code).map_err(serde::de::Error::custom)
    }
}

fn default_subject_source() -> String {
    "dc:subject".to_owned()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationCollection {
    name: String,
    collection_type: Option<String>,
    position: Option<String>,
}

impl PublicationCollection {
    pub fn new(name: impl AsRef<str>, collection_type: Option<String>, position: Option<String>) -> Result<Self, PublicationMetadataError> {
        Ok(Self { name: bounded_publication_text(name.as_ref())?, collection_type: collection_type.map(|value| bounded_publication_text(&value)).transpose()?, position: position.map(|value| bounded_publication_text(&value)).transpose()? })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn collection_type(&self) -> Option<&str> {
        self.collection_type.as_deref()
    }

    pub fn position(&self) -> Option<&str> {
        self.position.as_deref()
    }
}

impl<'de> Deserialize<'de> for PublicationCollection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            name: String,
            collection_type: Option<String>,
            position: Option<String>,
        }
        let encoded = Encoded::deserialize(deserializer)?;
        Self::new(encoded.name, encoded.collection_type, encoded.position).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationTitle {
    id: Option<String>,
    value: String,
    title_type: Option<String>,
    sort_as: Option<String>,
}

impl PublicationTitle {
    pub fn new(id: Option<String>, value: impl AsRef<str>, title_type: Option<String>, sort_as: Option<String>) -> Result<Self, PublicationMetadataError> {
        Ok(Self {
            id: id.map(|value| bounded_publication_text(&value)).transpose()?,
            value: bounded_publication_text(value.as_ref())?,
            title_type: title_type.map(|value| bounded_publication_text(&value)).transpose()?,
            sort_as: sort_as.map(|value| bounded_publication_text(&value)).transpose()?,
        })
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn title_type(&self) -> Option<&str> {
        self.title_type.as_deref()
    }

    pub fn sort_as(&self) -> Option<&str> {
        self.sort_as.as_deref()
    }
}

impl<'de> Deserialize<'de> for PublicationTitle {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            id: Option<String>,
            value: String,
            title_type: Option<String>,
            sort_as: Option<String>,
        }
        let encoded = Encoded::deserialize(deserializer)?;
        Self::new(encoded.id, encoded.value, encoded.title_type, encoded.sort_as).map_err(serde::de::Error::custom)
    }
}

/// Lossless storage for useful EPUB metadata we do not yet project into a
/// dedicated field. `refines` retains EPUB 3 relationships between values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicationProperty {
    id: Option<String>,
    property: String,
    value: String,
    refines: Option<String>,
    scheme: Option<String>,
    language: Option<String>,
}

impl PublicationProperty {
    pub fn new(id: Option<String>, property: impl AsRef<str>, value: impl AsRef<str>, refines: Option<String>, scheme: Option<String>, language: Option<String>) -> Result<Self, PublicationMetadataError> {
        Ok(Self {
            id: id.map(|value| bounded_publication_text(&value)).transpose()?,
            property: bounded_publication_text(property.as_ref())?,
            value: bounded_publication_text(value.as_ref())?,
            refines: refines.map(|value| bounded_publication_text(&value)).transpose()?,
            scheme: scheme.map(|value| bounded_publication_text(&value)).transpose()?,
            language: language.map(|value| bounded_publication_text(&value)).transpose()?,
        })
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn property(&self) -> &str {
        &self.property
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn refines(&self) -> Option<&str> {
        self.refines.as_deref()
    }

    pub fn scheme(&self) -> Option<&str> {
        self.scheme.as_deref()
    }

    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }
}

impl<'de> Deserialize<'de> for PublicationProperty {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            id: Option<String>,
            property: String,
            value: String,
            refines: Option<String>,
            scheme: Option<String>,
            language: Option<String>,
        }
        let encoded = Encoded::deserialize(deserializer)?;
        Self::new(encoded.id, encoded.property, encoded.value, encoded.refines, encoded.scheme, encoded.language).map_err(serde::de::Error::custom)
    }
}

/// Bibliographic metadata that accompanies the title, author credits, and
/// description carried alongside it in synchronized book state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationMetadata {
    #[serde(default)]
    pub identifiers: Vec<PublicationIdentifier>,
    #[serde(default)]
    pub publishers: Vec<PublisherCredit>,
    #[serde(default)]
    pub languages: Vec<LanguageTag>,
    #[serde(default)]
    pub dates: Vec<PublicationDate>,
    #[serde(default)]
    pub subjects: Vec<PublicationSubject>,
    #[serde(default)]
    pub collections: Vec<PublicationCollection>,
    #[serde(default)]
    pub alternate_titles: Vec<PublicationTitle>,
    #[serde(default)]
    pub rights: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub types: Vec<String>,
    #[serde(default)]
    pub formats: Vec<String>,
    #[serde(default)]
    pub relations: Vec<String>,
    #[serde(default)]
    pub coverage: Vec<String>,
    #[serde(default)]
    pub properties: Vec<PublicationProperty>,
}

/// One entry in a publication's own hierarchical navigation document. It is
/// shared by library ingestion and catalogue inspection; UI contracts only
/// project it at their process boundary.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationTocEntry {
    pub title: String,
    pub target: String,
    pub children: Vec<PublicationTocEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicationMetadataError;

impl fmt::Display for PublicationMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("publication metadata must contain bounded valid text")
    }
}

impl std::error::Error for PublicationMetadataError {}

const MAX_PUBLICATION_TEXT_BYTES: usize = 4096;

fn bounded_publication_text(value: &str) -> Result<String, PublicationMetadataError> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_PUBLICATION_TEXT_BYTES || value.chars().any(char::is_control) {
        return Err(PublicationMetadataError);
    }
    Ok(value.to_owned())
}

fn normalized_publication_text(value: &str) -> Result<String, PublicationMetadataError> {
    let value = bounded_publication_text(value)?;
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (normalized.len() <= MAX_PUBLICATION_TEXT_BYTES).then_some(normalized).ok_or(PublicationMetadataError)
}

fn canonical_identifier_value(scheme: &PublicationIdentifierScheme, value: &str) -> Option<String> {
    match scheme {
        PublicationIdentifierScheme::Isbn => canonical_isbn(value),
        PublicationIdentifierScheme::Doi => canonical_doi(value),
        PublicationIdentifierScheme::Uuid => canonical_uuid(value),
        _ => None,
    }
}
