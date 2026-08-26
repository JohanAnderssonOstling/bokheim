//! Bibliographic value types: publisher, language, identifier, contributor,
//! and other publication metadata carried by synchronized book state.
//!
//! These types are validated data, not synchronization logic; the merge
//! policy that treats a whole [`PublicationMetadata`] value as a single
//! last-writer-wins register lives with the mutation protocol, not here.

use crate::MarcRelatorCode;
pub use publication_model::{Collection, CollectionKind, Identifier, LanguageTag, LocalizedText, PublicationDate, PublicationProperty, PublicationSubject, PublicationTitle, Scheme, Scope};
use serde::{Deserialize, Serialize};
use std::fmt;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

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

/// Bibliographic metadata that accompanies the title, author credits, and
/// description carried alongside it in synchronized book state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationMetadata {
    #[serde(default)]
    pub identifiers: Vec<Identifier>,
    #[serde(default)]
    pub publishers: Vec<PublisherCredit>,
    #[serde(default)]
    pub languages: Vec<LanguageTag>,
    #[serde(default)]
    pub dates: Vec<PublicationDate>,
    #[serde(default)]
    pub subjects: Vec<PublicationSubject>,
    #[serde(default)]
    pub collections: Vec<Collection>,
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

impl From<publication_model::IdentifierError> for PublicationMetadataError {
    fn from(_: publication_model::IdentifierError) -> Self {
        Self
    }
}

impl From<publication_model::CollectionError> for PublicationMetadataError {
    fn from(_: publication_model::CollectionError) -> Self {
        Self
    }
}

impl From<publication_model::SubjectError> for PublicationMetadataError {
    fn from(_: publication_model::SubjectError) -> Self {
        Self
    }
}

impl From<publication_model::DateError> for PublicationMetadataError {
    fn from(_: publication_model::DateError) -> Self {
        Self
    }
}

impl From<publication_model::ValueError> for PublicationMetadataError {
    fn from(_: publication_model::ValueError) -> Self {
        Self
    }
}

impl From<publication_model::LanguageTagError> for PublicationMetadataError {
    fn from(_: publication_model::LanguageTagError) -> Self {
        Self
    }
}

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
