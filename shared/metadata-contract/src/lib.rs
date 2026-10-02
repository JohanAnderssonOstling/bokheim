pub mod wikidata;
pub use wikidata::*;
pub mod audible;
pub mod authority_subjects;
pub mod books;
pub mod identity_evidence;
pub mod matching;
pub mod subject_consensus;
pub mod year_consensus;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ISBNS_PER_REQUEST: usize = 256;
pub const MAX_EDITION_QUERIES_PER_REQUEST: usize = 128;

/// Bibliographic evidence for missing ISBNs or classification recovery after
/// an embedded ISBN returns no authority match.
/// The opaque query id lets callers correlate batched results without sending
/// a local book identifier to the metadata service.

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EditionIdentityQuery {
    pub query_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub book_year: Option<i32>,
}

/// A classification-only authority match does not assert an ISBN or OL identity.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AuthoritySubjectMatch {
    #[serde(default)]
    pub main_title: String,
    pub provider_id: String,
    pub record_ids: Vec<String>,
    pub title: String,
    pub authors: Vec<String>,
    pub classifications: Vec<Classification>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EditionIdentityResult {
    pub query_id: String,
    pub status: EditionIdentityStatus,
    /// Present only for a uniquely supported edition identity.
    pub matched: Option<EditionIdentityMatch>,
    /// Bounded alternatives for an ambiguous result. Empty for other statuses.
    #[serde(default)]
    pub candidates: Vec<EditionIdentityMatch>,
    /// True when a common title produced too many candidates to return safely.
    #[serde(default)]
    pub candidates_truncated: bool,
    #[serde(default)]
    pub authority_subjects: Option<AuthoritySubjectMatch>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EditionIdentityStatus {
    Matched,
    NoMatch,
    Ambiguous,
    InsufficientMetadata,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EditionIdentityMatch {
    pub canonical_isbn13: String,
    pub open_library_edition_id: String,
    pub open_library_work_id: Option<String>,
    pub title: String,
    pub work_title: Option<String>,
    pub title_match: EditionTitleMatch,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub book_year: Option<i32>,
    pub matched_publisher: bool,
    pub matched_book_year: bool,
    pub classifications: Vec<Classification>,
    #[serde(default)]
    pub subtitle: Option<String>,
}

impl EditionIdentityMatch {
    pub fn full_title(&self) -> String {
        match self.subtitle.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(subtitle) if !matching::bibliographic_match_key(&self.title).ends_with(&matching::bibliographic_match_key(subtitle)) => format!("{}: {}", self.title, subtitle),
            _ => self.title.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EditionTitleMatch {
    Edition,
    Work,
    Both,
}

/// Rich bibliographic lookup results for a batch of ISBNs.

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RichIsbnMetadataResult {
    pub requested_isbn: String,
    pub canonical_isbn13: Option<String>,
    pub status: LookupStatus,
    pub matches: Vec<RichWorkMetadataMatch>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RichWorkMetadataMatch {
    pub open_library_work_id: Option<String>,
    pub exact_edition_ids: Vec<String>,
    pub classifications: Vec<Classification>,
    pub authors: Vec<AuthorMetadata>,
    /// Plain-text description from the matched Open Library work.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub subjects: Vec<SubjectHeading>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SubjectHeading {
    pub source: String,
    pub authority: String,
    pub components: Vec<String>,
    pub authority_uri: Option<String>,
}

/// Merges Open Library evidence for every exact ISBN attached to one book.
/// Both the client library and authority enrichment use this policy so an
/// ambiguous ISBN can contribute consensus evidence without being mistaken
/// for a unique edition identity.
pub fn merge_rich_work_matches<'a>(matches: impl IntoIterator<Item = &'a RichWorkMetadataMatch>, similarity_keys: impl Fn(ClassificationScheme, &str) -> Vec<String>) -> RichWorkMetadataMatch {
    let mut work_ids = BTreeSet::new();
    let mut edition_ids = BTreeSet::new();
    let mut classifications = Vec::new();
    let mut authors = BTreeMap::<(u32, String), AuthorMetadata>::new();
    let mut descriptions = BTreeSet::new();
    let mut subjects = BTreeSet::new();
    for matched in matches {
        work_ids.extend(matched.open_library_work_id.iter().cloned());
        edition_ids.extend(matched.exact_edition_ids.iter().cloned());
        classifications.extend(matched.classifications.iter().cloned());
        for author in &matched.authors {
            authors
                .entry((author.position, author.identity_key()))
                .and_modify(|existing| {
                    if existing.name.is_none() {
                        existing.name = author.name.clone();
                    }
                    for identifier in &author.identifiers {
                        if !existing.identifiers.contains(identifier) {
                            existing.identifiers.push(identifier.clone());
                        }
                    }
                    existing.identifiers.sort();
                    existing.source = existing.source.min(author.source);
                })
                .or_insert_with(|| author.clone());
        }
        descriptions.extend(matched.description.iter().cloned());
        subjects.extend(matched.subjects.iter().cloned());
    }
    let classifications = filter_classification_outliers(classifications, similarity_keys);
    RichWorkMetadataMatch {
        open_library_work_id: work_ids.into_iter().next(),
        exact_edition_ids: edition_ids.into_iter().collect(),
        classifications,
        authors: authors.into_values().collect(),
        // Ambiguous ISBNs can point at unrelated works. Only expose a
        // description when all available Open Library evidence agrees.
        description: (descriptions.len() == 1).then(|| descriptions.into_iter().next().expect("one description exists")),
        subjects: subjects.into_iter().collect(),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SnapshotDescription {
    pub dump_date: String,
    pub imported_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IsbnClassificationResult {
    pub requested_isbn: String,
    pub canonical_isbn13: Option<String>,
    pub status: LookupStatus,
    pub matches: Vec<WorkClassificationMatch>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IsbnMetadataResult {
    pub requested_isbn: String,
    pub canonical_isbn13: Option<String>,
    pub status: LookupStatus,
    pub matches: Vec<WorkMetadataMatch>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupStatus {
    Matched,
    NoMatch,
    InvalidIsbn,
    Ambiguous,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkClassificationMatch {
    pub open_library_work_id: Option<String>,
    pub exact_edition_ids: Vec<String>,
    pub classifications: Vec<Classification>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkMetadataMatch {
    pub open_library_work_id: Option<String>,
    pub exact_edition_ids: Vec<String>,
    pub classifications: Vec<Classification>,
    pub authors: Vec<AuthorMetadata>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorMetadata {
    pub open_library_author_id: String,
    pub name: Option<String>,
    pub identifiers: Vec<AuthorIdentifier>,
    pub source: AuthorSource,
    pub position: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct AuthorIdentifier {
    pub authority: String,
    pub value: String,
}

impl AuthorMetadata {
    /// Wikidata-only authors have no Open Library identifier.
    pub fn identity_key(&self) -> String {
        if !self.open_library_author_id.is_empty() {
            return format!("openlibrary:{}", self.open_library_author_id);
        }
        self.identifiers.iter().find(|id| id.authority == "wikidata").map(|id| format!("wikidata:{}", id.value)).unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorSource {
    ExactEdition,
    Work,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Classification {
    // Persisted with bincode: even empty fields must retain their position.
    #[serde(default)]
    pub evidence: Vec<ClassificationEvidence>,
    pub scheme: ClassificationScheme,
    pub notation: String,
    pub source: ClassificationSource,
}

/// Provenance for a classification recovered across duplicate work records.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ClassificationEvidence {
    pub method: String,
    pub isbn13: String,
    pub open_library_work_id: String,
    pub open_library_edition_id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationScheme {
    LibraryOfCongress,
    /// Dewey Decimal classification.
    DeweyDecimal,
    Bisac,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationSource {
    ExactEdition,
    Work,
}

/// Normalizes a bibliographic call number to the stable class notation used
/// for cross-edition consensus. LCC cutters and item years must not create
/// separate votes. Dewey segmentation marks are normalized.
pub fn classification_consensus_notation(scheme: ClassificationScheme, notation: &str) -> Option<String> {
    match scheme {
        ClassificationScheme::DeweyDecimal => {
            let value = notation.trim().replace('/', "");
            let bytes = value.as_bytes();
            let valid = bytes.len() >= 3 && bytes[..3].iter().all(u8::is_ascii_digit) && (bytes.len() == 3 || (bytes[3] == b'.' && bytes.len() > 4 && bytes[4..].iter().all(u8::is_ascii_digit)));
            valid.then_some(value)
        }
        ClassificationScheme::LibraryOfCongress => {
            let value = notation.trim();
            let bytes = value.as_bytes();
            let mut cursor = 0;
            while cursor < bytes.len() && cursor < 3 && bytes[cursor].is_ascii_alphabetic() {
                cursor += 1;
            }
            if cursor == 0 || bytes.get(cursor).is_some_and(u8::is_ascii_alphabetic) {
                return None;
            }
            let letters = value[..cursor].to_ascii_uppercase();
            while bytes.get(cursor).is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'\'') {
                cursor += 1;
            }
            let number_start = cursor;
            while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
                cursor += 1;
            }
            if cursor == number_start {
                return None;
            }
            if bytes.get(cursor) == Some(&b'.') {
                let decimal_start = cursor + 1;
                let mut decimal_end = decimal_start;
                while bytes.get(decimal_end).is_some_and(u8::is_ascii_digit) {
                    decimal_end += 1;
                }
                if decimal_end > decimal_start {
                    cursor = decimal_end;
                }
            }
            Some(format!("{letters}{}", &value[number_start..cursor]))
        }
        ClassificationScheme::Bisac => {
            let value = notation.trim().to_ascii_uppercase();
            (value.len() == 9 && value.as_bytes()[..3].iter().all(u8::is_ascii_alphabetic) && value.as_bytes()[3..].iter().all(u8::is_ascii_digit)).then_some(value)
        }
    }
}

/// Accumulates exact-edition classification support across alternative ISBN
/// matches without allowing a repeated work-wide union to manufacture votes.
#[derive(Debug, Default)]
pub struct EditionClassificationConsensus {
    support: BTreeMap<(ClassificationScheme, String), BTreeSet<String>>,
}

impl EditionClassificationConsensus {
    /// Removes exact-edition classifications from alternative matches and
    /// records each normalized classification once per independently queried
    /// edition group. Work-level evidence remains on the matches.
    pub fn record_alternative_matches(&mut self, matches: &mut [RichWorkMetadataMatch], requested_isbn: &str) {
        for matched in matches {
            let edition_group = if matched.exact_edition_ids.is_empty() {
                format!("isbn:{requested_isbn}")
            } else {
                let mut edition_ids = matched.exact_edition_ids.clone();
                edition_ids.sort();
                edition_ids.dedup();
                edition_ids.join(",")
            };
            matched.classifications.retain(|classification| {
                if classification.source == ClassificationSource::ExactEdition {
                    if let Some(notation) = classification_consensus_notation(classification.scheme, &classification.notation) {
                        self.support.entry((classification.scheme, notation)).or_default().insert(edition_group.clone());
                    }
                    false
                } else {
                    true
                }
            });
            for author in &mut matched.authors {
                if author.source == AuthorSource::ExactEdition {
                    author.source = AuthorSource::Work;
                }
            }
        }
    }

    /// Returns normalized classifications supported by at least two distinct
    /// edition groups.
    pub fn classifications(self) -> Vec<Classification> {
        self.support.into_iter().filter_map(|((scheme, notation), edition_groups)| (edition_groups.len() >= 2).then_some(Classification { evidence: Vec::new(), scheme, notation, source: ClassificationSource::Work })).collect()
    }
}

/// Returns normalized classifications supported by at least two exact
/// editions. Duplicate formatting variants within one edition count once.
pub fn exact_edition_classification_consensus<'a, E, C>(editions: E) -> Vec<Classification>
where
    E: IntoIterator<Item = C>,
    C: IntoIterator<Item = &'a (ClassificationScheme, String)>,
{
    let mut support = BTreeMap::<(ClassificationScheme, String), usize>::new();
    for classifications in editions {
        let normalized = classifications.into_iter().filter_map(|(scheme, notation)| classification_consensus_notation(*scheme, notation).map(|notation| (*scheme, notation))).collect::<BTreeSet<_>>();
        for classification in normalized {
            *support.entry(classification).or_default() += 1;
        }
    }
    support.into_iter().filter_map(|((scheme, notation), votes)| (votes >= 2).then_some(Classification { evidence: Vec::new(), scheme, notation, source: ClassificationSource::Work })).collect()
}

/// Keeps classification consensus across editions while removing small,
/// distant taxonomy neighborhoods from a sufficiently large evidence set.
/// Exact-edition classifications are always retained. Unmapped values are
/// retained conservatively.
pub fn filter_classification_outliers(classifications: impl IntoIterator<Item = Classification>, similarity_keys: impl Fn(ClassificationScheme, &str) -> Vec<String>) -> Vec<Classification> {
    let mut evidence = BTreeMap::<(ClassificationScheme, String), BTreeSet<ClassificationEvidence>>::new();
    let mut unique = BTreeMap::<(ClassificationScheme, String), ClassificationSource>::new();
    for classification in classifications {
        evidence.entry((classification.scheme, classification.notation.clone())).or_default().extend(classification.evidence);
        unique.entry((classification.scheme, classification.notation)).and_modify(|source| *source = (*source).min(classification.source)).or_insert(classification.source);
    }

    let mut classification_keys = BTreeMap::<(ClassificationScheme, String), Vec<String>>::new();
    let mut family_counts = BTreeMap::<String, usize>::new();
    let mut mapped_total = 0_usize;
    for (scheme, notation) in unique.keys() {
        let keys = similarity_keys(*scheme, notation).into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>();
        if !keys.is_empty() {
            mapped_total += 1;
            for key in &keys {
                *family_counts.entry(key.clone()).or_default() += 1;
            }
        }
        classification_keys.insert((*scheme, notation.clone()), keys);
    }

    let strongest = family_counts.values().copied().max().unwrap_or_default();
    let minimum_support = 2_usize.max(mapped_total.div_ceil(20));

    unique
        .into_iter()
        .filter_map(|((scheme, notation), source)| {
            let keep = if source == ClassificationSource::ExactEdition {
                true
            } else {
                let keys = classification_keys.get(&(scheme, notation.clone())).map(Vec::as_slice).unwrap_or_default();
                keys.is_empty() || mapped_total < 6 || strongest < 2 || keys.iter().any(|key| family_counts.get(key).copied().unwrap_or_default() >= minimum_support)
            };
            keep.then(|| Classification { evidence: evidence.remove(&(scheme, notation.clone())).unwrap_or_default().into_iter().collect(), scheme, notation, source })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifications_round_trip_through_binary_storage() {
        for scheme in [ClassificationScheme::LibraryOfCongress, ClassificationScheme::DeweyDecimal, ClassificationScheme::Bisac] {
            for evidence in [Vec::new(), vec![ClassificationEvidence { method: "exact".to_owned(), isbn13: "9780306406157".to_owned(), open_library_work_id: "OL10W".to_owned(), open_library_edition_id: "OL1M".to_owned() }]] {
                let value = Classification { evidence, scheme, notation: "QA76.73".to_owned(), source: ClassificationSource::Work };
                let bytes = wire::encode(&value).unwrap();
                assert_eq!(wire::decode::<Classification>(&bytes, MAX_RESPONSE_BYTES).unwrap(), value);
            }
        }
    }

    #[test]
    fn classification_consensus_drops_small_distant_families_but_keeps_exact_evidence() {
        let mut classifications = (1..=8).map(|number| Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation: format!("PA40{number}"), source: ClassificationSource::Work }).collect::<Vec<_>>();
        classifications.extend([
            Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation: "B580".to_owned(), source: ClassificationSource::ExactEdition },
            Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation: "PQ3434".to_owned(), source: ClassificationSource::Work },
            Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation: "HQ75.6".to_owned(), source: ClassificationSource::Work },
        ]);

        let filtered = filter_classification_outliers(classifications, |_, notation| {
            let key = if notation.starts_with("PA") {
                "Humanities / Literature / Classical Greek & Latin Literatures"
            } else if notation.starts_with("B") {
                "Humanities / Philosophy / History, Traditions & Regions"
            } else if notation.starts_with("PQ") {
                "Humanities / Literature / Romance-Language Literatures"
            } else {
                "Social Sciences / Society / Social Topics"
            };
            vec![key.to_owned()]
        });
        assert_eq!(filtered.len(), 9);
        assert!(filtered.iter().any(|classification| classification.notation == "B580"));
        assert!(!filtered.iter().any(|classification| classification.notation == "PQ3434"));
        assert!(!filtered.iter().any(|classification| classification.notation == "HQ75.6"));
    }

    #[test]
    fn exact_edition_consensus_keeps_multiple_supported_branches_per_scheme() {
        let editions = [
            [(ClassificationScheme::LibraryOfCongress, "QA76.6".to_owned()), (ClassificationScheme::DeweyDecimal, "005.1".to_owned())].into_iter().collect::<BTreeSet<_>>(),
            [(ClassificationScheme::LibraryOfCongress, "QA76.6.A1 2001".to_owned()), (ClassificationScheme::DeweyDecimal, "005.1".to_owned())].into_iter().collect(),
            [(ClassificationScheme::LibraryOfCongress, "PN1993".to_owned()), (ClassificationScheme::DeweyDecimal, "791.43".to_owned())].into_iter().collect(),
            [(ClassificationScheme::LibraryOfCongress, "PN1993.A2".to_owned()), (ClassificationScheme::DeweyDecimal, "791.43".to_owned())].into_iter().collect(),
            [(ClassificationScheme::LibraryOfCongress, "B580".to_owned()), (ClassificationScheme::DeweyDecimal, "999".to_owned())].into_iter().collect(),
        ];

        let retained = exact_edition_classification_consensus(editions.iter().map(BTreeSet::iter));
        let notations = retained.iter().map(|classification| classification.notation.as_str()).collect::<BTreeSet<_>>();
        assert!(notations.contains("QA76.6"));
        assert!(notations.contains("PN1993"));
        assert!(notations.contains("005.1"));
        assert!(notations.contains("791.43"));
        assert!(!notations.contains("B580"));
        assert!(!notations.contains("999"));
    }

    #[test]
    fn classification_consensus_normalizes_item_specific_call_numbers() {
        assert_eq!(classification_consensus_notation(ClassificationScheme::LibraryOfCongress, "PA4025.A5 R6 1999").as_deref(), Some("PA4025"));
        assert_eq!(classification_consensus_notation(ClassificationScheme::LibraryOfCongress, "PA4025.A5W56 2018eb").as_deref(), Some("PA4025"));
        assert_eq!(classification_consensus_notation(ClassificationScheme::DeweyDecimal, "883/.01").as_deref(), Some("883.01"));
    }

    #[test]
    fn alternative_edition_consensus_removes_singletons_and_demotes_other_evidence() {
        let matched = |work: &str, edition: &str, notation: &str| RichWorkMetadataMatch {
            open_library_work_id: Some(work.to_owned()),
            exact_edition_ids: vec![edition.to_owned()],
            classifications: vec![Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation: notation.to_owned(), source: ClassificationSource::ExactEdition }],
            authors: vec![AuthorMetadata { open_library_author_id: "OL1A".to_owned(), name: None, identifiers: Vec::new(), source: AuthorSource::ExactEdition, position: 0 }],
            description: None,
            subjects: Vec::new(),
        };
        let mut consensus = EditionClassificationConsensus::default();
        let mut first = vec![matched("OL1W", "OL1M", "PR4034.P7 1995")];
        let mut second = vec![matched("OL2W", "OL2M", "PR4034 .P7 2006")];
        let mut singleton = vec![matched("OL3W", "OL3M", "PS3558.A4758 P75t 1999")];
        consensus.record_alternative_matches(&mut first, "9780000000001");
        consensus.record_alternative_matches(&mut second, "9780000000002");
        consensus.record_alternative_matches(&mut singleton, "9780000000003");

        assert!(first[0].classifications.is_empty());
        assert_eq!(first[0].authors[0].source, AuthorSource::Work);
        assert_eq!(consensus.classifications(), vec![Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation: "PR4034".to_owned(), source: ClassificationSource::Work }]);
    }

    #[test]
    fn rich_match_merging_combines_split_work_records() {
        let first = RichWorkMetadataMatch {
            open_library_work_id: Some("OL1W".to_owned()),
            exact_edition_ids: vec!["OL1M".to_owned()],
            classifications: vec![Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation: "GN496".to_owned(), source: ClassificationSource::ExactEdition }],
            authors: Vec::new(),
            description: None,
            subjects: Vec::new(),
        };
        let second = RichWorkMetadataMatch {
            open_library_work_id: Some("OL2W".to_owned()),
            exact_edition_ids: vec!["OL2M".to_owned()],
            classifications: vec![Classification { evidence: Vec::new(), scheme: ClassificationScheme::DeweyDecimal, notation: "306".to_owned(), source: ClassificationSource::Work }],
            authors: Vec::new(),
            description: None,
            subjects: Vec::new(),
        };

        let merged = merge_rich_work_matches([&first, &second], |_, _| Vec::new());
        assert_eq!(merged.exact_edition_ids, ["OL1M", "OL2M"]);
        assert_eq!(merged.classifications.len(), 2);
        assert!(merged.classifications.iter().any(|c| c.scheme == ClassificationScheme::DeweyDecimal));
    }
}
