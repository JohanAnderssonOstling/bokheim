//! Classification provenance is typed in memory. Its historical string form is
//! retained at Serde boundaries so existing answer databases and clients remain
//! readable. Unknown methods round-trip without gaining a known method's policy.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClassificationProvider {
    LibraryOfCongress,
    Wikidata,
    OpenLibraryWork,
}

impl ClassificationProvider {
    pub fn from_provider_id(value: &str) -> Option<Self> {
        match value {
            "library_of_congress" => Some(Self::LibraryOfCongress),
            "wikidata" => Some(Self::Wikidata),
            "openlibrary_work" => Some(Self::OpenLibraryWork),
            _ => None,
        }
    }

    fn provider_id(self) -> &'static str {
        match self {
            Self::LibraryOfCongress => "library_of_congress",
            Self::Wikidata => "wikidata",
            Self::OpenLibraryWork => "openlibrary_work",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClassificationMethod {
    TitleAuthor {
        provider: ClassificationProvider,
        record_id: String,
    },
    LibraryOfCongressIsbn {
        record_id: String,
    },
    LibraryThingIsbn {
        work_id: String,
    },
    WikidataDirect {
        entity: String,
        property: String,
    },
    DirectEditionLcc {
        notation: String,
    },
    /// Older answers carried a display label, rather than weighted support.
    SiblingSubjectConsensus {
        support: String,
    },
    OpenLibraryDeclaredWorkLcc,
    VerifiedMatchingEditions,
    VerifiedYearVariants,
    VerifiedShortCreditEdition,
    VerifiedFilenameSurnameTitle,
    VerifiedUniqueTitle,
    LocalPrintIsbnVerifiedTitleAuthor,
    LocalIsbnReference,
    LocalTitleAuthorAfterIsbnMiss,
    Unknown(String),
}

impl ClassificationMethod {
    fn stored_name(&self) -> Cow<'_, str> {
        match self {
            Self::TitleAuthor { provider, record_id } => format!("{}_title_author:{record_id}", provider.provider_id()).into(),
            Self::LibraryOfCongressIsbn { record_id } => format!("loc_api_isbn:{record_id}").into(),
            Self::LibraryThingIsbn { work_id } => format!("librarything_isbn:{work_id}").into(),
            Self::WikidataDirect { entity, property } => format!("wikidata:direct:{entity}:{property}").into(),
            Self::DirectEditionLcc { notation } => format!("direct_edition_lcc:{notation}").into(),
            Self::SiblingSubjectConsensus { support } => format!("verified_sibling_subject_consensus:{support}").into(),
            Self::OpenLibraryDeclaredWorkLcc => "openlibrary_declared_work_lcc".into(),
            Self::VerifiedMatchingEditions => "verified_matching_edition_classifications".into(),
            Self::VerifiedYearVariants => "verified_year_variant_classifications".into(),
            Self::VerifiedShortCreditEdition => "verified_short_credit_edition_classification".into(),
            Self::VerifiedFilenameSurnameTitle => "verified_filename_surname_title_classification".into(),
            Self::VerifiedUniqueTitle => "verified_unique_title_classification".into(),
            Self::LocalPrintIsbnVerifiedTitleAuthor => "local_bibliographic_print_isbn_verified_title_author".into(),
            Self::LocalIsbnReference => "local_bibliographic_isbn_reference".into(),
            Self::LocalTitleAuthorAfterIsbnMiss => "local_title_author_after_isbn_miss".into(),
            Self::Unknown(value) => value.as_str().into(),
        }
    }

    fn from_stored(value: String) -> Self {
        match value.as_str() {
            "openlibrary_declared_work_lcc" => return Self::OpenLibraryDeclaredWorkLcc,
            "verified_matching_edition_classifications" => return Self::VerifiedMatchingEditions,
            "verified_year_variant_classifications" => return Self::VerifiedYearVariants,
            "verified_short_credit_edition_classification" => return Self::VerifiedShortCreditEdition,
            "verified_filename_surname_title_classification" => return Self::VerifiedFilenameSurnameTitle,
            "verified_unique_title_classification" => return Self::VerifiedUniqueTitle,
            "local_bibliographic_print_isbn_verified_title_author" => return Self::LocalPrintIsbnVerifiedTitleAuthor,
            "local_bibliographic_isbn_reference" => return Self::LocalIsbnReference,
            "local_title_author_after_isbn_miss" => return Self::LocalTitleAuthorAfterIsbnMiss,
            _ => {}
        }
        for provider in [ClassificationProvider::LibraryOfCongress, ClassificationProvider::Wikidata, ClassificationProvider::OpenLibraryWork] {
            if let Some(record_id) = value.strip_prefix(&format!("{}_title_author:", provider.provider_id())) {
                return Self::TitleAuthor { provider, record_id: record_id.into() };
            }
        }
        if let Some(record_id) = value.strip_prefix("loc_api_isbn:") {
            return Self::LibraryOfCongressIsbn { record_id: record_id.into() };
        }
        if let Some(work_id) = value.strip_prefix("librarything_isbn:") {
            return Self::LibraryThingIsbn { work_id: work_id.into() };
        }
        if let Some((entity, property)) = value.strip_prefix("wikidata:direct:").and_then(|value| value.split_once(':')) {
            return Self::WikidataDirect { entity: entity.into(), property: property.into() };
        }
        if let Some(notation) = value.strip_prefix("direct_edition_lcc:") {
            return Self::DirectEditionLcc { notation: notation.into() };
        }
        if let Some(support) = value.strip_prefix("verified_sibling_subject_consensus:") {
            return Self::SiblingSubjectConsensus { support: support.into() };
        }
        Self::Unknown(value)
    }
}

impl fmt::Display for ClassificationMethod {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.stored_name())
    }
}

impl Serialize for ClassificationMethod {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ClassificationMethod {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from_stored)
    }
}

// Evidence sets retain the lexical order used by their persisted string form.
impl Ord for ClassificationMethod {
    fn cmp(&self, other: &Self) -> Ordering {
        self.stored_name().cmp(&other.stored_name()).then_with(|| match (self, other) {
            (Self::WikidataDirect { entity: left, property: left_property }, Self::WikidataDirect { entity: right, property: right_property }) => (left, left_property).cmp(&(right, right_property)),
            _ => matches!(self, Self::Unknown(_)).cmp(&matches!(other, Self::Unknown(_))),
        })
    }
}

impl PartialOrd for ClassificationMethod {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
