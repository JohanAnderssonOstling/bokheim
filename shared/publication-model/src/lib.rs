//! Bibliographic values shared by catalogue and local-library metadata.

mod canonical;
mod credits;
mod marc;

pub use canonical::{canonical_doi, canonical_isbn, canonical_uuid};
pub use credits::{
    agent_id_from_migration_seed, AgentId, AuthorId, BookMetadataState, Contributor, ContributorId, PublicationMetadata, PublicationMetadataError, PublicationTocEntry, PublisherCredit, PublisherId, PublisherMatchKey, PublisherName,
};
pub use marc::{marc_relator_code_from_metadata_role, MarcRelatorCode, AUTHOR_MARC_RELATOR_CODE, CONTRIBUTOR_MARC_RELATOR_CODE};
use serde::{Deserialize, Serialize};
use std::fmt;

const MAX_VALUE_BYTES: usize = 4096;
const LEGACY_SOURCE_SCHEME: &str = "bokheim:source";
const LEGACY_SCOPE_SCHEME: &str = "bokheim:scope";
const LEGACY_PROVENANCE_SCHEME: &str = "bokheim:provenance";

/// Returns a year only when the supplied date evidence contains exactly one
/// plausible four-digit publication year.
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
    Publication,
    Edition,
    Work,
}

impl Scope {
    fn label(self) -> &'static str {
        match self {
            Self::Publication => "publication",
            Self::Edition => "edition",
            Self::Work => "work",
        }
    }

    fn from_label(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "publication" => Some(Self::Publication),
            "edition" => Some(Self::Edition),
            "work" => Some(Self::Work),
            _ => None,
        }
    }
}

/// One bibliographic identifier. Catalogue-entry keys are deliberately not
/// represented here; they belong to the catalogue listing identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Identifier {
    pub value: String,
    pub scheme: Scheme,
    pub scope: Scope,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalizedText {
    pub language: String,
    pub value: String,
}

/// A validated, canonical BCP-47 language tag shared by catalogue and local
/// library metadata. Underscores are repaired at ingestion because they are a
/// common POSIX-style provider error; serialized values always use BCP-47
/// hyphens and canonical subtag casing.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct LanguageTag(String);

impl LanguageTag {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, LanguageTagError> {
        let value = value.as_ref().trim().replace('_', "-");
        let tag = language_tags::LanguageTag::parse(&value).map_err(|_| LanguageTagError)?;
        tag.validate().map_err(|_| LanguageTagError)?;
        Ok(Self(tag.canonicalize().unwrap_or(tag).into_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for LanguageTag {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<&str> for LanguageTag {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Display for LanguageTag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LanguageTag {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LanguageTagError;

impl fmt::Display for LanguageTagError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("language must be a valid BCP-47 tag")
    }
}

impl std::error::Error for LanguageTagError {}

/// One date assertion carried by a publication, together with the metadata
/// source that supplied it. `event` distinguishes publication, modification,
/// copyright, and other date semantics without normalizing away the raw value.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct PublicationDate {
    pub id: Option<String>,
    pub value: String,
    pub event: Option<String>,
    pub source: String,
}

impl PublicationDate {
    pub fn new(id: Option<String>, value: impl AsRef<str>, event: Option<String>, source: impl AsRef<str>) -> Result<Self, DateError> {
        Ok(Self {
            id: id.map(|value| bounded(&value).map_err(|_| DateError)).transpose()?,
            value: bounded(value.as_ref()).map_err(|_| DateError)?,
            event: event.map(|value| bounded(&value).map_err(|_| DateError)).transpose()?,
            source: bounded(source.as_ref()).map_err(|_| DateError)?,
        })
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

    pub fn source(&self) -> &str {
        &self.source
    }
}

impl<'de> Deserialize<'de> for PublicationDate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            id: Option<String>,
            value: String,
            event: Option<String>,
            source: String,
        }

        let encoded = Encoded::deserialize(deserializer)?;
        Self::new(encoded.id, encoded.value, encoded.event, encoded.source).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateError;

impl fmt::Display for DateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("publication date metadata is invalid")
    }
}

impl std::error::Error for DateError {}

/// A secondary or specially typed title exactly as supplied by a publication.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct PublicationTitle {
    pub id: Option<String>,
    pub value: String,
    pub title_type: Option<String>,
    pub sort_as: Option<String>,
    pub source: String,
}

impl PublicationTitle {
    pub fn new(id: Option<String>, value: impl AsRef<str>, title_type: Option<String>, sort_as: Option<String>, source: impl AsRef<str>) -> Result<Self, ValueError> {
        Ok(Self {
            id: id.map(|value| bounded(&value).map_err(|_| ValueError)).transpose()?,
            value: bounded(value.as_ref()).map_err(|_| ValueError)?,
            title_type: title_type.map(|value| bounded(&value).map_err(|_| ValueError)).transpose()?,
            sort_as: sort_as.map(|value| bounded(&value).map_err(|_| ValueError)).transpose()?,
            source: bounded(source.as_ref()).map_err(|_| ValueError)?,
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

    pub fn source(&self) -> &str {
        &self.source
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
            source: String,
        }

        let encoded = Encoded::deserialize(deserializer)?;
        Self::new(encoded.id, encoded.value, encoded.title_type, encoded.sort_as, encoded.source).map_err(serde::de::Error::custom)
    }
}

/// Lossless metadata not yet projected into a dedicated publication field.
/// `refines` retains EPUB relationships and `source` retains provenance.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct PublicationProperty {
    pub id: Option<String>,
    pub property: String,
    pub value: String,
    pub refines: Option<String>,
    pub scheme: Option<String>,
    pub language: Option<String>,
    pub source: String,
}

impl PublicationProperty {
    #[allow(clippy::too_many_arguments)]
    pub fn new(id: Option<String>, property: impl AsRef<str>, value: impl AsRef<str>, refines: Option<String>, scheme: Option<String>, language: Option<String>, source: impl AsRef<str>) -> Result<Self, ValueError> {
        Ok(Self {
            id: id.map(|value| bounded(&value).map_err(|_| ValueError)).transpose()?,
            property: bounded(property.as_ref()).map_err(|_| ValueError)?,
            value: bounded(value.as_ref()).map_err(|_| ValueError)?,
            refines: refines.map(|value| bounded(&value).map_err(|_| ValueError)).transpose()?,
            scheme: scheme.map(|value| bounded(&value).map_err(|_| ValueError)).transpose()?,
            language: language.map(|value| bounded(&value).map_err(|_| ValueError)).transpose()?,
            source: bounded(source.as_ref()).map_err(|_| ValueError)?,
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

    pub fn source(&self) -> &str {
        &self.source
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
            source: String,
        }

        let encoded = Encoded::deserialize(deserializer)?;
        Self::new(encoded.id, encoded.property, encoded.value, encoded.refines, encoded.scheme, encoded.language, encoded.source).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueError;

impl fmt::Display for ValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("publication value metadata is invalid")
    }
}

impl std::error::Error for ValueError {}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Audience {
    pub value: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgeRange {
    pub value: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicationImage {
    pub url: String,
    pub media_type: Option<String>,
    pub relations: Vec<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AggregateRating {
    pub value: String,
    pub rating_count: Option<u64>,
    pub review_count: Option<u64>,
    pub best_rating: Option<String>,
    pub worst_rating: Option<String>,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InteractionStatistic {
    pub interaction_type: String,
    pub count: u64,
    pub service: Option<String>,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PopularityRank {
    pub position: u64,
    pub source: String,
}

/// Exact subject evidence supplied by a publication or catalogue. Taxonomy
/// projection never rewrites this value; it produces separate assignments.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct PublicationSubject {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub name_localizations: Vec<LocalizedText>,
    #[serde(default)]
    pub sort_name: Option<String>,
    #[serde(default)]
    pub sort_name_localizations: Vec<LocalizedText>,
    pub source: String,
    pub authority: Option<String>,
    pub code: Option<String>,
}

impl<'de> Deserialize<'de> for PublicationSubject {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            id: Option<String>,
            name: String,
            #[serde(default)]
            name_localizations: Vec<LocalizedText>,
            #[serde(default)]
            sort_name: Option<String>,
            #[serde(default)]
            sort_name_localizations: Vec<LocalizedText>,
            source: String,
            authority: Option<String>,
            code: Option<String>,
        }

        let value = Encoded::deserialize(deserializer)?;
        Self::with_details(value.id, value.name, value.name_localizations, value.sort_name, value.sort_name_localizations, value.source, value.authority, value.code).map_err(serde::de::Error::custom)
    }
}

impl PublicationSubject {
    pub fn new(id: Option<String>, name: impl AsRef<str>, source: impl AsRef<str>, authority: Option<String>, code: Option<String>) -> Result<Self, SubjectError> {
        Self::with_details(id, name, Vec::new(), None, Vec::new(), source, authority, code)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_details(
        id: Option<String>, name: impl AsRef<str>, name_localizations: Vec<LocalizedText>, sort_name: Option<String>, sort_name_localizations: Vec<LocalizedText>, source: impl AsRef<str>, authority: Option<String>, code: Option<String>,
    ) -> Result<Self, SubjectError> {
        Ok(Self {
            id: id.map(|value| bounded(&value).map_err(|_| SubjectError)).transpose()?,
            name: bounded(name.as_ref()).map_err(|_| SubjectError)?,
            name_localizations,
            sort_name: sort_name.map(|value| bounded(&value).map_err(|_| SubjectError)).transpose()?,
            sort_name_localizations,
            source: bounded(source.as_ref()).map_err(|_| SubjectError)?,
            authority: authority.map(|value| bounded(&value).map_err(|_| SubjectError)).transpose()?,
            code: code.map(|value| bounded(&value).map_err(|_| SubjectError)).transpose()?,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubjectError;

impl fmt::Display for SubjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("subject metadata is invalid")
    }
}

impl std::error::Error for SubjectError {}

impl LocalizedText {
    pub fn new(language: impl AsRef<str>, value: impl AsRef<str>) -> Result<Self, CollectionError> {
        Ok(Self { language: bounded(language.as_ref()).map_err(|_| CollectionError)?, value: bounded(value.as_ref()).map_err(|_| CollectionError)? })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionKind {
    Series,
    Collection,
    Other(String),
}

impl CollectionKind {
    pub fn from_label(value: impl AsRef<str>) -> Result<Self, CollectionError> {
        let value = bounded(value.as_ref()).map_err(|_| CollectionError)?;
        Ok(match value.to_ascii_lowercase().as_str() {
            "series" => Self::Series,
            "collection" => Self::Collection,
            _ => Self::Other(value),
        })
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Series => "series",
            Self::Collection => "collection",
            Self::Other(value) => value,
        }
    }
}

/// Membership of a publication in a bibliographic series or collection.
/// Catalogue navigation groups are deliberately represented elsewhere.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Collection {
    pub name: String,
    pub name_localizations: Vec<LocalizedText>,
    pub sort_name: Option<String>,
    pub sort_name_localizations: Vec<LocalizedText>,
    pub identifiers: Vec<String>,
    pub kind: Option<CollectionKind>,
    pub position: Option<String>,
    pub source: String,
}

impl Collection {
    pub fn new(name: impl AsRef<str>, kind: Option<CollectionKind>, position: Option<String>, source: impl AsRef<str>) -> Result<Self, CollectionError> {
        Self::with_details(name, Vec::new(), None, Vec::new(), Vec::new(), kind, position, source)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_details(
        name: impl AsRef<str>, name_localizations: Vec<LocalizedText>, sort_name: Option<String>, sort_name_localizations: Vec<LocalizedText>, identifiers: Vec<String>, kind: Option<CollectionKind>, position: Option<String>,
        source: impl AsRef<str>,
    ) -> Result<Self, CollectionError> {
        Ok(Self {
            name: bounded(name.as_ref()).map_err(|_| CollectionError)?,
            name_localizations,
            sort_name: sort_name.map(|value| bounded(&value).map_err(|_| CollectionError)).transpose()?,
            sort_name_localizations,
            identifiers: identifiers.into_iter().map(|value| bounded(&value).map_err(|_| CollectionError)).collect::<Result<_, _>>()?,
            kind,
            position: position.map(|value| bounded(&value).map_err(|_| CollectionError)).transpose()?,
            source: bounded(source.as_ref()).map_err(|_| CollectionError)?,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn collection_type(&self) -> Option<&str> {
        self.kind.as_ref().map(CollectionKind::label)
    }

    pub fn position(&self) -> Option<&str> {
        self.position.as_deref()
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

impl<'de> Deserialize<'de> for Collection {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            name: String,
            #[serde(default)]
            name_localizations: Vec<LocalizedText>,
            #[serde(default)]
            sort_name: Option<String>,
            #[serde(default)]
            sort_name_localizations: Vec<LocalizedText>,
            #[serde(default)]
            identifiers: Vec<String>,
            #[serde(default, alias = "collection_type")]
            kind: Option<CollectionKind>,
            #[serde(default)]
            position: Option<String>,
            #[serde(default = "legacy_collection_source")]
            source: String,
        }

        let encoded = Encoded::deserialize(deserializer)?;
        Self::with_details(encoded.name, encoded.name_localizations, encoded.sort_name, encoded.sort_name_localizations, encoded.identifiers, encoded.kind, encoded.position, encoded.source).map_err(serde::de::Error::custom)
    }
}

fn legacy_collection_source() -> String {
    "legacy:collection".to_owned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectionError;

impl fmt::Display for CollectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("collection metadata is invalid")
    }
}

impl std::error::Error for CollectionError {}

impl Identifier {
    pub fn new(value: impl AsRef<str>, scheme: Scheme, scope: Scope, source: impl AsRef<str>) -> Result<Self, IdentifierError> {
        Ok(Self { value: bounded(value.as_ref())?, scheme, scope, source: bounded(source.as_ref())? })
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

    pub fn source(&self) -> &str {
        &self.source
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

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LegacyAnnotation {
    value: String,
    scheme: Option<String>,
}

#[derive(Serialize)]
struct HumanIdentifier<'a> {
    value: &'a str,
    scheme: &'a Scheme,
    scope: Scope,
    source: &'a str,
}

#[derive(Deserialize)]
struct HumanIdentifierOwned {
    value: String,
    scheme: Scheme,
    #[serde(default)]
    scope: Scope,
    #[serde(default = "legacy_source")]
    source: String,
    #[serde(default)]
    identifier_types: Vec<LegacyAnnotation>,
}

#[derive(Serialize, Deserialize)]
struct BinaryIdentifier {
    value: String,
    scheme: Scheme,
    identifier_types: Vec<LegacyAnnotation>,
}

impl Serialize for Identifier {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return HumanIdentifier { value: &self.value, scheme: &self.scheme, scope: self.scope, source: &self.source }.serialize(serializer);
        }
        let mut identifier_types = vec![LegacyAnnotation { value: self.source.clone(), scheme: Some(LEGACY_SOURCE_SCHEME.to_owned()) }];
        if self.scope != Scope::Publication {
            identifier_types.push(LegacyAnnotation { value: self.scope.label().to_owned(), scheme: Some(LEGACY_SCOPE_SCHEME.to_owned()) });
        }
        BinaryIdentifier { value: self.value.clone(), scheme: self.scheme.clone(), identifier_types }.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Identifier {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let encoded = HumanIdentifierOwned::deserialize(deserializer)?;
            let source = source_from_annotations(&encoded.identifier_types).unwrap_or(encoded.source);
            let scope = scope_from_annotations(&encoded.identifier_types).unwrap_or(encoded.scope);
            return Self::new(encoded.value, encoded.scheme, scope, source).map_err(serde::de::Error::custom);
        }
        let encoded = BinaryIdentifier::deserialize(deserializer)?;
        let source = source_from_annotations(&encoded.identifier_types).unwrap_or_else(legacy_source);
        let scope = scope_from_annotations(&encoded.identifier_types).unwrap_or_default();
        Self::new(encoded.value, encoded.scheme, scope, source).map_err(serde::de::Error::custom)
    }
}

fn source_from_annotations(values: &[LegacyAnnotation]) -> Option<String> {
    values.iter().find(|value| value.scheme.as_deref() == Some(LEGACY_SOURCE_SCHEME)).or_else(|| values.iter().find(|value| value.scheme.as_deref() == Some(LEGACY_PROVENANCE_SCHEME))).map(|value| value.value.clone())
}

fn scope_from_annotations(values: &[LegacyAnnotation]) -> Option<Scope> {
    values.iter().find(|value| value.scheme.as_deref() == Some(LEGACY_SCOPE_SCHEME)).and_then(|value| Scope::from_label(&value.value))
}

fn legacy_source() -> String {
    "legacy:identifier".to_owned()
}

fn bounded(value: &str) -> Result<String, IdentifierError> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_VALUE_BYTES || value.chars().any(char::is_control) {
        return Err(IdentifierError);
    }
    Ok(value.to_owned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentifierError;

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("identifier value or source is invalid")
    }
}

impl std::error::Error for IdentifierError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_shared_identifiers() {
        let identifier = Identifier::new("urn:isbn:978-1-23456-789-7", Scheme::Isbn, Scope::Edition, "epub:package:identifier").unwrap();
        assert_eq!(identifier.canonical_value().as_deref(), Some("9781234567897"));
    }

    #[test]
    fn language_tags_use_one_validated_canonical_representation() {
        assert_eq!(LanguageTag::parse("SV_se").unwrap().as_str(), "sv-SE");
        assert_eq!(LanguageTag::parse("zh-hant-tw").unwrap().as_str(), "zh-Hant-TW");
        assert!(LanguageTag::parse("en--US").is_err());

        let encoded = serde_json::to_string(&LanguageTag::parse("iw-IL").unwrap()).unwrap();
        assert_eq!(encoded, r#""he-IL""#);
        assert_eq!(serde_json::from_str::<LanguageTag>(&encoded).unwrap().as_str(), "he-IL");
    }

    #[test]
    fn reads_legacy_human_identifier_evidence() {
        let identifier: Identifier = serde_json::from_str(r#"{"value":"9781234567897","scheme":"isbn","identifier_types":[{"value":"xmp","scheme":"bokheim:provenance"}]}"#).unwrap();
        assert_eq!(identifier.source(), "xmp");
        assert_eq!(identifier.scope(), Scope::Publication);
    }

    #[test]
    fn reads_and_writes_the_legacy_binary_shape() {
        let legacy = BinaryIdentifier { value: "9781234567897".to_owned(), scheme: Scheme::Isbn, identifier_types: vec![LegacyAnnotation { value: "xmp".to_owned(), scheme: Some("bokheim:provenance".to_owned()) }] };
        let encoded = bincode::serde::encode_to_vec(&legacy, bincode::config::standard()).unwrap();
        let (decoded, consumed): (Identifier, usize) = bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded.source(), "xmp");

        let shared = Identifier::new("OL1M", Scheme::Other("openlibrary".to_owned()), Scope::Edition, "openlibrary:identity-match").unwrap();
        let encoded = bincode::serde::encode_to_vec(&shared, bincode::config::standard()).unwrap();
        let (legacy, consumed): (BinaryIdentifier, usize) = bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(consumed, encoded.len());
        assert!(legacy.identifier_types.iter().any(|value| value.scheme.as_deref() == Some(LEGACY_SOURCE_SCHEME) && value.value == "openlibrary:identity-match"));
        assert!(legacy.identifier_types.iter().any(|value| value.scheme.as_deref() == Some(LEGACY_SCOPE_SCHEME) && value.value == "edition"));
    }

    #[test]
    fn collection_retains_rich_membership_evidence() {
        let collection = Collection::with_details(
            "The Earthsea Cycle",
            vec![LocalizedText::new("sv", "Övärlden").unwrap()],
            Some("Earthsea Cycle, The".to_owned()),
            Vec::new(),
            vec!["urn:series:earthsea".to_owned()],
            Some(CollectionKind::Series),
            Some("2".to_owned()),
            "opds2:metadata.belongsTo.series",
        )
        .unwrap();
        let encoded = serde_json::to_string(&collection).unwrap();
        let decoded: Collection = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, collection);
        assert_eq!(decoded.collection_type(), Some("series"));
    }

    #[test]
    fn date_retains_event_and_source_in_json_and_binary_storage() {
        let date = PublicationDate::new(Some("pub-date".to_owned()), "2026-08-22", Some("publication".to_owned()), "opds2:metadata.published").unwrap();

        let json = serde_json::to_string(&date).unwrap();
        assert_eq!(serde_json::from_str::<PublicationDate>(&json).unwrap(), date);

        let encoded = bincode::serde::encode_to_vec(&date, bincode::config::standard()).unwrap();
        let (decoded, consumed): (PublicationDate, usize) = bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded, date);
    }

    #[test]
    fn title_and_property_retain_source_in_json_and_binary_storage() {
        let title = PublicationTitle::new(None, "The Tombs of Atuan", Some("alternative".to_owned()), Some("Tombs of Atuan, The".to_owned()), "epub:package:title").unwrap();
        let property = PublicationProperty::new(None, "belongs-to-collection", "Earthsea", None, None, Some("en".to_owned()), "epub:package:meta").unwrap();

        assert_eq!(serde_json::from_str::<PublicationTitle>(&serde_json::to_string(&title).unwrap()).unwrap(), title);
        let encoded = bincode::serde::encode_to_vec(&property, bincode::config::standard()).unwrap();
        let (decoded, consumed): (PublicationProperty, usize) = bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded, property);
    }

    #[test]
    fn subject_retains_rich_evidence_in_json_and_binary_storage() {
        let subject = PublicationSubject::with_details(
            Some("https://id.kb.se/term/saogf/Historiska-romaner".to_owned()),
            "Historiska romaner",
            vec![LocalizedText::new("en", "Historical fiction").unwrap()],
            Some("Romaner, historiska".to_owned()),
            Vec::new(),
            "opds2:subject",
            Some("saogf".to_owned()),
            Some("Historiska romaner".to_owned()),
        )
        .unwrap();

        let json = serde_json::to_string(&subject).unwrap();
        assert_eq!(serde_json::from_str::<PublicationSubject>(&json).unwrap(), subject);

        let encoded = bincode::serde::encode_to_vec(&subject, bincode::config::standard()).unwrap();
        let (decoded, consumed): (PublicationSubject, usize) = bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded, subject);
    }
}
