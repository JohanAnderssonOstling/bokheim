use prost::Message;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const MEDIA_TYPE_V2: &str = "application/vnd.bokheim.metadata+protobuf; version=2";
pub const MEDIA_TYPE_V2_BASE: &str = "application/vnd.bokheim.metadata+protobuf";
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ISBNS_PER_REQUEST: usize = 256;
pub const MAX_EDITION_QUERIES_PER_REQUEST: usize = 128;

mod protobuf_v2 {
    // Generated from proto/metadata_v2.proto by prost-build 0.14.4.
    include!("bokheim.metadata.v2.rs");
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClassificationRequest {
    pub isbns: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClassificationResponse {
    pub snapshot: SnapshotDescription,
    pub results: Vec<IsbnClassificationResult>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MetadataEnrichmentRequest {
    pub isbns: Vec<String>,
}

/// Bibliographic evidence used only when a publication has no embedded ISBN.
/// The opaque query id lets callers correlate batched results without sending
/// a local book identifier to the metadata service.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EditionIdentityRequest {
    pub queries: Vec<EditionIdentityQuery>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EditionIdentityQuery {
    pub query_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub publication_year: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EditionIdentityResponse {
    pub snapshot: SnapshotDescription,
    pub results: Vec<EditionIdentityResult>,
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
    pub publication_year: Option<i32>,
    pub matched_publisher: bool,
    pub matched_publication_year: bool,
    pub classifications: Vec<Classification>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EditionTitleMatch {
    Edition,
    Work,
    Both,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MetadataEnrichmentResponse {
    pub snapshot: SnapshotDescription,
    pub results: Vec<IsbnMetadataResult>,
}

/// Rich bibliographic enrichment. This is deliberately a separate wire value
/// from [`MetadataEnrichmentResponse`], so deployed version-1 clients remain
/// byte-compatible while clients adopt entity navigation independently.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RichMetadataEnrichmentResponse {
    pub snapshot: SnapshotDescription,
    pub results: Vec<RichIsbnMetadataResult>,
    /// Resolved entities are interned once per response. References in results
    /// and parent links use the stable Wikidata identifier.
    pub places: Vec<PlaceEntity>,
    pub people: Vec<PersonEntity>,
}

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
    pub places: Vec<EntityReference>,
    pub people: Vec<EntityReference>,
    pub popularity: Option<WorkPopularity>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkPopularity {
    pub want_to_read_count: u64,
    pub currently_reading_count: u64,
    pub already_read_count: u64,
    pub rating_count: u64,
    pub rating_sum: u64,
}

impl WorkPopularity {
    pub fn reading_log_count(&self) -> u64 {
        self.want_to_read_count.saturating_add(self.currently_reading_count).saturating_add(self.already_read_count)
    }
}

/// Merges Open Library evidence for every exact ISBN attached to one book.
/// Both the client library and catalogue enrichment use this policy so an
/// ambiguous ISBN can contribute consensus evidence without being mistaken
/// for a unique edition identity.
pub fn merge_rich_work_matches<'a>(matches: impl IntoIterator<Item = &'a RichWorkMetadataMatch>, similarity_keys: impl Fn(ClassificationScheme, &str) -> Vec<String>) -> RichWorkMetadataMatch {
    let mut work_ids = BTreeSet::new();
    let mut edition_ids = BTreeSet::new();
    let mut classifications = BTreeMap::<(ClassificationScheme, String), ClassificationSource>::new();
    let mut authors = BTreeMap::<(u32, String), AuthorMetadata>::new();
    let mut places = BTreeMap::<(Option<String>, String), EntitySource>::new();
    let mut people = BTreeMap::<(Option<String>, String), EntitySource>::new();
    let mut popularity = None;
    for matched in matches {
        work_ids.extend(matched.open_library_work_id.iter().cloned());
        edition_ids.extend(matched.exact_edition_ids.iter().cloned());
        for classification in &matched.classifications {
            classifications.entry((classification.scheme, classification.notation.clone())).and_modify(|source| *source = (*source).min(classification.source)).or_insert(classification.source);
        }
        for author in &matched.authors {
            authors
                .entry((author.position, author.open_library_author_id.clone()))
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
        for reference in &matched.places {
            places.entry((reference.wikidata_id.clone(), reference.name.clone())).and_modify(|source| *source = (*source).min(reference.source)).or_insert(reference.source);
        }
        for reference in &matched.people {
            people.entry((reference.wikidata_id.clone(), reference.name.clone())).and_modify(|source| *source = (*source).min(reference.source)).or_insert(reference.source);
        }
        if matched.popularity.as_ref().is_some_and(|candidate| popularity.as_ref().is_none_or(|current: &WorkPopularity| (candidate.reading_log_count(), candidate.rating_count) > (current.reading_log_count(), current.rating_count))) {
            popularity = matched.popularity.clone();
        }
    }
    let classifications = filter_classification_outliers(classifications.into_iter().map(|((scheme, notation), source)| Classification { scheme, notation, source }), similarity_keys);
    RichWorkMetadataMatch {
        open_library_work_id: work_ids.into_iter().next(),
        exact_edition_ids: edition_ids.into_iter().collect(),
        classifications,
        authors: authors.into_values().collect(),
        places: places.into_iter().map(|((wikidata_id, name), source)| EntityReference { wikidata_id, name, source }).collect(),
        people: people.into_iter().map(|((wikidata_id, name), source)| EntityReference { wikidata_id, name, source }).collect(),
        popularity,
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct EntityReference {
    /// Absent when the dump evidence could not be resolved conservatively.
    pub wikidata_id: Option<String>,
    /// The original Open Library heading, preserved as bibliographic evidence.
    pub name: String,
    pub source: EntitySource,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntitySource {
    ExactEdition,
    Work,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlaceEntity {
    pub wikidata_id: String,
    pub label: String,
    pub aliases: Vec<String>,
    /// Immediate administrative/containment parents. This is a graph rather
    /// than a tree; the presentation layer may choose a preferred path.
    pub parent_ids: Vec<String>,
    pub country_ids: Vec<String>,
    pub latitude_e7: Option<i32>,
    pub longitude_e7: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersonEntity {
    pub wikidata_id: String,
    pub label: String,
    pub aliases: Vec<String>,
    pub birth_year: Option<i32>,
    pub death_year: Option<i32>,
    pub fictional: bool,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorSource {
    ExactEdition,
    Work,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Classification {
    pub scheme: ClassificationScheme,
    pub notation: String,
    pub source: ClassificationSource,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationScheme {
    LibraryOfCongress,
    DeweyDecimal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationSource {
    ExactEdition,
    Work,
}

/// Normalizes a bibliographic call number to the stable class notation used
/// for cross-edition consensus. LCC cutters, item years, and DDC punctuation
/// variants must not create separate votes.
pub fn classification_consensus_notation(scheme: ClassificationScheme, notation: &str) -> Option<String> {
    match scheme {
        ClassificationScheme::DeweyDecimal => {
            let compact = notation.chars().filter(|character| !character.is_whitespace() && *character != '/').collect::<String>();
            let valid = if compact.len() == 3 {
                compact.bytes().all(|byte| byte.is_ascii_digit())
            } else {
                compact.split_once('.').is_some_and(|(whole, decimal)| whole.len() == 3 && whole.bytes().all(|byte| byte.is_ascii_digit()) && !decimal.is_empty() && decimal.bytes().all(|byte| byte.is_ascii_digit()))
            };
            valid.then_some(compact)
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
    }
}

/// Keeps classification consensus across editions while removing small,
/// distant taxonomy neighborhoods from a sufficiently large evidence set.
/// Exact-edition classifications are always retained. Unmapped values are
/// retained conservatively.
pub fn filter_classification_outliers(classifications: impl IntoIterator<Item = Classification>, similarity_keys: impl Fn(ClassificationScheme, &str) -> Vec<String>) -> Vec<Classification> {
    let mut unique = BTreeMap::<(ClassificationScheme, String), ClassificationSource>::new();
    for classification in classifications {
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
            keep.then_some(Classification { scheme, notation, source })
        })
        .collect()
}

#[derive(Debug, Eq, PartialEq)]
pub enum WireError {
    TooLarge { limit: usize },
    Encode(String),
    Decode(String),
}

impl fmt::Display for WireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { limit } => write!(formatter, "metadata body exceeds the {limit}-byte limit"),
            Self::Encode(reason) => write!(formatter, "could not encode metadata body: {reason}"),
            Self::Decode(reason) => write!(formatter, "could not decode metadata body: {reason}"),
        }
    }
}

impl std::error::Error for WireError {}

pub fn encode_v2_request(value: &MetadataEnrichmentRequest) -> Result<Vec<u8>, WireError> {
    let message = protobuf_v2::EnrichmentRequest { isbns: value.isbns.clone() };
    encode_protobuf(&message)
}

pub fn decode_v2_request(body: &[u8], limit: usize) -> Result<MetadataEnrichmentRequest, WireError> {
    check_size(body, limit)?;
    let message = protobuf_v2::EnrichmentRequest::decode(body).map_err(|error| WireError::Decode(error.to_string()))?;
    Ok(MetadataEnrichmentRequest { isbns: message.isbns })
}

pub fn encode_v2_response(value: &RichMetadataEnrichmentResponse) -> Result<Vec<u8>, WireError> {
    encode_protobuf(&protobuf_v2::EnrichmentResponse::from(value))
}

pub fn decode_v2_response(body: &[u8], limit: usize) -> Result<RichMetadataEnrichmentResponse, WireError> {
    check_size(body, limit)?;
    protobuf_v2::EnrichmentResponse::decode(body).map_err(|error| WireError::Decode(error.to_string()))?.try_into()
}

pub fn encode_v2_edition_identity_request(value: &EditionIdentityRequest) -> Result<Vec<u8>, WireError> {
    encode_protobuf(&protobuf_v2::EditionIdentityRequest {
        queries: value
            .queries
            .iter()
            .map(|query| protobuf_v2::EditionIdentityQuery { query_id: query.query_id.clone(), title: query.title.clone(), authors: query.authors.clone(), publishers: query.publishers.clone(), publication_year: query.publication_year })
            .collect(),
    })
}

pub fn decode_v2_edition_identity_request(body: &[u8], limit: usize) -> Result<EditionIdentityRequest, WireError> {
    check_size(body, limit)?;
    let value = protobuf_v2::EditionIdentityRequest::decode(body).map_err(|error| WireError::Decode(error.to_string()))?;
    Ok(EditionIdentityRequest {
        queries: value.queries.into_iter().map(|query| EditionIdentityQuery { query_id: query.query_id, title: query.title, authors: query.authors, publishers: query.publishers, publication_year: query.publication_year }).collect(),
    })
}

pub fn encode_v2_edition_identity_response(value: &EditionIdentityResponse) -> Result<Vec<u8>, WireError> {
    encode_protobuf(&protobuf_v2::EditionIdentityResponse {
        snapshot: Some(protobuf_v2::Snapshot { dump_date: value.snapshot.dump_date.clone(), imported_at_ms: value.snapshot.imported_at_ms }),
        results: value.results.iter().map(protobuf_edition_identity_result).collect(),
    })
}

pub fn decode_v2_edition_identity_response(body: &[u8], limit: usize) -> Result<EditionIdentityResponse, WireError> {
    check_size(body, limit)?;
    let value = protobuf_v2::EditionIdentityResponse::decode(body).map_err(|error| WireError::Decode(error.to_string()))?;
    let snapshot = value.snapshot.ok_or_else(|| WireError::Decode("protobuf edition identity response omitted snapshot".to_owned()))?;
    Ok(EditionIdentityResponse { snapshot: SnapshotDescription { dump_date: snapshot.dump_date, imported_at_ms: snapshot.imported_at_ms }, results: value.results.into_iter().map(domain_edition_identity_result).collect::<Result<_, _>>()? })
}

fn protobuf_edition_identity_result(value: &EditionIdentityResult) -> protobuf_v2::EditionIdentityResult {
    protobuf_v2::EditionIdentityResult {
        query_id: value.query_id.clone(),
        status: match value.status {
            EditionIdentityStatus::Matched => 1,
            EditionIdentityStatus::NoMatch => 2,
            EditionIdentityStatus::Ambiguous => 3,
            EditionIdentityStatus::InsufficientMetadata => 4,
        },
        matched: value.matched.as_ref().map(protobuf_edition_identity_match),
        candidates: value.candidates.iter().map(protobuf_edition_identity_match).collect(),
        candidates_truncated: value.candidates_truncated,
    }
}

fn protobuf_edition_identity_match(value: &EditionIdentityMatch) -> protobuf_v2::EditionIdentityMatch {
    protobuf_v2::EditionIdentityMatch {
        canonical_isbn13: value.canonical_isbn13.clone(),
        open_library_edition_id: value.open_library_edition_id.clone(),
        open_library_work_id: value.open_library_work_id.clone(),
        title: value.title.clone(),
        work_title: value.work_title.clone(),
        title_match: match value.title_match {
            EditionTitleMatch::Edition => 1,
            EditionTitleMatch::Work => 2,
            EditionTitleMatch::Both => 3,
        },
        authors: value.authors.clone(),
        publishers: value.publishers.clone(),
        publication_year: value.publication_year,
        matched_publisher: value.matched_publisher,
        matched_publication_year: value.matched_publication_year,
        classifications: value.classifications.iter().map(protobuf_classification).collect(),
    }
}

fn protobuf_classification(value: &Classification) -> protobuf_v2::Classification {
    protobuf_v2::Classification {
        scheme: match value.scheme {
            ClassificationScheme::LibraryOfCongress => 1,
            ClassificationScheme::DeweyDecimal => 2,
        },
        notation: value.notation.clone(),
        source: match value.source {
            ClassificationSource::ExactEdition => 1,
            ClassificationSource::Work => 2,
        },
    }
}

fn domain_classification(value: protobuf_v2::Classification) -> Result<Classification, WireError> {
    Ok(Classification {
        scheme: match value.scheme {
            1 => ClassificationScheme::LibraryOfCongress,
            2 => ClassificationScheme::DeweyDecimal,
            scheme => return Err(invalid_enum("classification scheme", scheme)),
        },
        notation: value.notation,
        source: match value.source {
            1 => ClassificationSource::ExactEdition,
            2 => ClassificationSource::Work,
            source => return Err(invalid_enum("classification source", source)),
        },
    })
}

fn domain_edition_identity_result(value: protobuf_v2::EditionIdentityResult) -> Result<EditionIdentityResult, WireError> {
    Ok(EditionIdentityResult {
        query_id: value.query_id,
        status: match value.status {
            1 => EditionIdentityStatus::Matched,
            2 => EditionIdentityStatus::NoMatch,
            3 => EditionIdentityStatus::Ambiguous,
            4 => EditionIdentityStatus::InsufficientMetadata,
            status => return Err(invalid_enum("edition identity status", status)),
        },
        matched: value.matched.map(domain_edition_identity_match).transpose()?,
        candidates: value.candidates.into_iter().map(domain_edition_identity_match).collect::<Result<_, _>>()?,
        candidates_truncated: value.candidates_truncated,
    })
}

fn domain_edition_identity_match(value: protobuf_v2::EditionIdentityMatch) -> Result<EditionIdentityMatch, WireError> {
    Ok(EditionIdentityMatch {
        canonical_isbn13: value.canonical_isbn13,
        open_library_edition_id: value.open_library_edition_id,
        open_library_work_id: value.open_library_work_id,
        title: value.title,
        work_title: value.work_title,
        title_match: match value.title_match {
            1 => EditionTitleMatch::Edition,
            2 => EditionTitleMatch::Work,
            3 => EditionTitleMatch::Both,
            title_match => return Err(invalid_enum("edition title match", title_match)),
        },
        authors: value.authors,
        publishers: value.publishers,
        publication_year: value.publication_year,
        matched_publisher: value.matched_publisher,
        matched_publication_year: value.matched_publication_year,
        classifications: value.classifications.into_iter().map(domain_classification).collect::<Result<_, _>>()?,
    })
}

fn encode_protobuf(message: &impl Message) -> Result<Vec<u8>, WireError> {
    let mut body = Vec::with_capacity(message.encoded_len());
    message.encode(&mut body).map_err(|error| WireError::Encode(error.to_string()))?;
    Ok(body)
}

fn check_size(body: &[u8], limit: usize) -> Result<(), WireError> {
    if body.len() > limit {
        Err(WireError::TooLarge { limit })
    } else {
        Ok(())
    }
}

pub fn is_binary_media_type_v2(value: &str) -> bool {
    let mut fields = value.split(';');
    if !fields.next().is_some_and(|value| value.trim().eq_ignore_ascii_case(MEDIA_TYPE_V2_BASE)) {
        return false;
    }
    fields.any(|field| field.split_once('=').is_some_and(|(name, value)| name.trim().eq_ignore_ascii_case("version") && value.trim() == "2"))
}

impl From<&RichMetadataEnrichmentResponse> for protobuf_v2::EnrichmentResponse {
    fn from(value: &RichMetadataEnrichmentResponse) -> Self {
        Self {
            snapshot: Some(protobuf_v2::Snapshot { dump_date: value.snapshot.dump_date.clone(), imported_at_ms: value.snapshot.imported_at_ms }),
            results: value.results.iter().map(protobuf_result).collect(),
            places: value.places.iter().map(protobuf_place).collect(),
            people: value.people.iter().map(protobuf_person).collect(),
        }
    }
}

fn protobuf_result(value: &RichIsbnMetadataResult) -> protobuf_v2::IsbnResult {
    protobuf_v2::IsbnResult { requested_isbn: value.requested_isbn.clone(), canonical_isbn13: value.canonical_isbn13.clone(), status: lookup_status_number(value.status), matches: value.matches.iter().map(protobuf_match).collect() }
}

fn protobuf_match(value: &RichWorkMetadataMatch) -> protobuf_v2::WorkMatch {
    protobuf_v2::WorkMatch {
        open_library_work_id: value.open_library_work_id.clone(),
        exact_edition_ids: value.exact_edition_ids.clone(),
        classifications: value
            .classifications
            .iter()
            .map(|classification| protobuf_v2::Classification {
                scheme: match classification.scheme {
                    ClassificationScheme::LibraryOfCongress => 1,
                    ClassificationScheme::DeweyDecimal => 2,
                },
                notation: classification.notation.clone(),
                source: match classification.source {
                    ClassificationSource::ExactEdition => 1,
                    ClassificationSource::Work => 2,
                },
            })
            .collect(),
        authors: value
            .authors
            .iter()
            .map(|author| protobuf_v2::Author {
                open_library_author_id: author.open_library_author_id.clone(),
                name: author.name.clone(),
                identifiers: author.identifiers.iter().map(|identifier| protobuf_v2::AuthorIdentifier { authority: identifier.authority.clone(), value: identifier.value.clone() }).collect(),
                source: match author.source {
                    AuthorSource::ExactEdition => 1,
                    AuthorSource::Work => 2,
                },
                position: author.position,
            })
            .collect(),
        places: value.places.iter().map(protobuf_reference).collect(),
        people: value.people.iter().map(protobuf_reference).collect(),
        popularity: value.popularity.as_ref().map(|popularity| protobuf_v2::WorkPopularity {
            want_to_read_count: popularity.want_to_read_count,
            currently_reading_count: popularity.currently_reading_count,
            already_read_count: popularity.already_read_count,
            rating_count: popularity.rating_count,
            rating_sum: popularity.rating_sum,
        }),
    }
}

fn protobuf_reference(value: &EntityReference) -> protobuf_v2::EntityReference {
    protobuf_v2::EntityReference {
        wikidata_id: value.wikidata_id.clone(),
        name: value.name.clone(),
        source: match value.source {
            EntitySource::ExactEdition => 1,
            EntitySource::Work => 2,
        },
    }
}

fn protobuf_place(value: &PlaceEntity) -> protobuf_v2::PlaceEntity {
    protobuf_v2::PlaceEntity {
        wikidata_id: value.wikidata_id.clone(),
        label: value.label.clone(),
        aliases: value.aliases.clone(),
        parent_ids: value.parent_ids.clone(),
        country_ids: value.country_ids.clone(),
        latitude_e7: value.latitude_e7,
        longitude_e7: value.longitude_e7,
    }
}

fn protobuf_person(value: &PersonEntity) -> protobuf_v2::PersonEntity {
    protobuf_v2::PersonEntity { wikidata_id: value.wikidata_id.clone(), label: value.label.clone(), aliases: value.aliases.clone(), birth_year: value.birth_year, death_year: value.death_year, fictional: value.fictional }
}

fn lookup_status_number(value: LookupStatus) -> i32 {
    match value {
        LookupStatus::Matched => 1,
        LookupStatus::NoMatch => 2,
        LookupStatus::InvalidIsbn => 3,
        LookupStatus::Ambiguous => 4,
    }
}

impl TryFrom<protobuf_v2::EnrichmentResponse> for RichMetadataEnrichmentResponse {
    type Error = WireError;

    fn try_from(value: protobuf_v2::EnrichmentResponse) -> Result<Self, Self::Error> {
        let snapshot = value.snapshot.ok_or_else(|| WireError::Decode("protobuf response omitted snapshot".to_owned()))?;
        Ok(Self {
            snapshot: SnapshotDescription { dump_date: snapshot.dump_date, imported_at_ms: snapshot.imported_at_ms },
            results: value.results.into_iter().map(domain_result).collect::<Result<_, _>>()?,
            places: value
                .places
                .into_iter()
                .map(|place| PlaceEntity {
                    wikidata_id: place.wikidata_id,
                    label: place.label,
                    aliases: place.aliases,
                    parent_ids: place.parent_ids,
                    country_ids: place.country_ids,
                    latitude_e7: place.latitude_e7,
                    longitude_e7: place.longitude_e7,
                })
                .collect(),
            people: value
                .people
                .into_iter()
                .map(|person| PersonEntity { wikidata_id: person.wikidata_id, label: person.label, aliases: person.aliases, birth_year: person.birth_year, death_year: person.death_year, fictional: person.fictional })
                .collect(),
        })
    }
}

fn domain_result(value: protobuf_v2::IsbnResult) -> Result<RichIsbnMetadataResult, WireError> {
    Ok(RichIsbnMetadataResult { requested_isbn: value.requested_isbn, canonical_isbn13: value.canonical_isbn13, status: domain_lookup_status(value.status)?, matches: value.matches.into_iter().map(domain_match).collect::<Result<_, _>>()? })
}

fn domain_match(value: protobuf_v2::WorkMatch) -> Result<RichWorkMetadataMatch, WireError> {
    Ok(RichWorkMetadataMatch {
        open_library_work_id: value.open_library_work_id,
        exact_edition_ids: value.exact_edition_ids,
        classifications: value
            .classifications
            .into_iter()
            .map(|classification| {
                Ok(Classification {
                    scheme: match classification.scheme {
                        1 => ClassificationScheme::LibraryOfCongress,
                        2 => ClassificationScheme::DeweyDecimal,
                        value => return Err(invalid_enum("classification scheme", value)),
                    },
                    notation: classification.notation,
                    source: match classification.source {
                        1 => ClassificationSource::ExactEdition,
                        2 => ClassificationSource::Work,
                        value => return Err(invalid_enum("classification source", value)),
                    },
                })
            })
            .collect::<Result<_, _>>()?,
        authors: value
            .authors
            .into_iter()
            .map(|author| {
                Ok(AuthorMetadata {
                    open_library_author_id: author.open_library_author_id,
                    name: author.name,
                    identifiers: author.identifiers.into_iter().map(|identifier| AuthorIdentifier { authority: identifier.authority, value: identifier.value }).collect(),
                    source: match author.source {
                        1 => AuthorSource::ExactEdition,
                        2 => AuthorSource::Work,
                        value => return Err(invalid_enum("author source", value)),
                    },
                    position: author.position,
                })
            })
            .collect::<Result<_, _>>()?,
        places: value.places.into_iter().map(domain_reference).collect::<Result<_, _>>()?,
        people: value.people.into_iter().map(domain_reference).collect::<Result<_, _>>()?,
        popularity: value.popularity.map(|popularity| WorkPopularity {
            want_to_read_count: popularity.want_to_read_count,
            currently_reading_count: popularity.currently_reading_count,
            already_read_count: popularity.already_read_count,
            rating_count: popularity.rating_count,
            rating_sum: popularity.rating_sum,
        }),
    })
}

fn domain_reference(value: protobuf_v2::EntityReference) -> Result<EntityReference, WireError> {
    Ok(EntityReference {
        wikidata_id: value.wikidata_id,
        name: value.name,
        source: match value.source {
            1 => EntitySource::ExactEdition,
            2 => EntitySource::Work,
            source => return Err(invalid_enum("entity source", source)),
        },
    })
}

fn domain_lookup_status(value: i32) -> Result<LookupStatus, WireError> {
    match value {
        1 => Ok(LookupStatus::Matched),
        2 => Ok(LookupStatus::NoMatch),
        3 => Ok(LookupStatus::InvalidIsbn),
        4 => Ok(LookupStatus::Ambiguous),
        value => Err(invalid_enum("lookup status", value)),
    }
}

fn invalid_enum(name: &str, value: i32) -> WireError {
    WireError::Decode(format!("protobuf {name} has unsupported value {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification_consensus_drops_small_distant_families_but_keeps_exact_evidence() {
        let mut classifications = (1..=8).map(|number| Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: format!("PA40{number}"), source: ClassificationSource::Work }).collect::<Vec<_>>();
        classifications.extend([
            Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "B580".to_owned(), source: ClassificationSource::ExactEdition },
            Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "PQ3434".to_owned(), source: ClassificationSource::Work },
            Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "HQ75.6".to_owned(), source: ClassificationSource::Work },
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
    fn protobuf_v2_round_trip_preserves_rich_entities() {
        let value = RichMetadataEnrichmentResponse {
            snapshot: SnapshotDescription { dump_date: "2026-08-01".to_owned(), imported_at_ms: 42 },
            results: vec![RichIsbnMetadataResult {
                requested_isbn: "9780306406157".to_owned(),
                canonical_isbn13: Some("9780306406157".to_owned()),
                status: LookupStatus::Matched,
                matches: vec![RichWorkMetadataMatch {
                    open_library_work_id: Some("OL10W".to_owned()),
                    exact_edition_ids: vec!["OL1M".to_owned()],
                    classifications: vec![Classification { scheme: ClassificationScheme::DeweyDecimal, notation: "883".to_owned(), source: ClassificationSource::Work }],
                    authors: Vec::new(),
                    places: vec![EntityReference { wikidata_id: Some("Q90".to_owned()), name: "Paris".to_owned(), source: EntitySource::Work }],
                    people: Vec::new(),
                    popularity: Some(WorkPopularity { want_to_read_count: 12, currently_reading_count: 3, already_read_count: 8, rating_count: 4, rating_sum: 17 }),
                }],
            }],
            places: vec![PlaceEntity {
                wikidata_id: "Q90".to_owned(),
                label: "Paris".to_owned(),
                aliases: Vec::new(),
                parent_ids: vec!["Q161741".to_owned()],
                country_ids: vec!["Q142".to_owned()],
                latitude_e7: Some(488_566_000),
                longitude_e7: Some(23_522_000),
            }],
            people: Vec::new(),
        };
        let mut encoded = encode_v2_response(&value).unwrap();
        // A future scalar field 99 must be ignored by this version.
        encoded.extend_from_slice(&[0x98, 0x06, 0x01]);
        let decoded = decode_v2_response(&encoded, MAX_RESPONSE_BYTES).unwrap();
        assert_eq!(decoded.snapshot.dump_date, "2026-08-01");
        assert_eq!(decoded.places, value.places);
        assert_eq!(decoded.results[0].matches[0].classifications, value.results[0].matches[0].classifications);
        assert_eq!(decoded.results[0].matches[0].places, value.results[0].matches[0].places);
        assert_eq!(decoded.results[0].matches[0].popularity, value.results[0].matches[0].popularity);

        let request = MetadataEnrichmentRequest { isbns: vec!["9780306406157".to_owned()] };
        assert_eq!(decode_v2_request(&encode_v2_request(&request).unwrap(), MAX_REQUEST_BYTES).unwrap().isbns, request.isbns);
    }

    #[test]
    fn protobuf_v2_round_trip_preserves_edition_review_candidates() {
        let candidate = EditionIdentityMatch {
            canonical_isbn13: "9780306406157".to_owned(),
            open_library_edition_id: "OL1M".to_owned(),
            open_library_work_id: Some("OL10W".to_owned()),
            title: "Edition title".to_owned(),
            work_title: Some("Work title".to_owned()),
            title_match: EditionTitleMatch::Work,
            authors: vec!["Alex Smith".to_owned()],
            publishers: vec!["North Wind Press".to_owned()],
            publication_year: Some(2004),
            matched_publisher: true,
            matched_publication_year: false,
            classifications: vec![Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: "QA76.6".to_owned(), source: ClassificationSource::Work }],
        };
        let value = EditionIdentityResponse {
            snapshot: SnapshotDescription { dump_date: "2026-08-01".to_owned(), imported_at_ms: 42 },
            results: vec![EditionIdentityResult { query_id: "book-1".to_owned(), status: EditionIdentityStatus::Ambiguous, matched: None, candidates: vec![candidate.clone()], candidates_truncated: true }],
        };
        let decoded = decode_v2_edition_identity_response(&encode_v2_edition_identity_response(&value).unwrap(), MAX_RESPONSE_BYTES).unwrap();
        assert_eq!(decoded.results[0].status, EditionIdentityStatus::Ambiguous);
        assert_eq!(decoded.results[0].candidates[0].title_match, EditionTitleMatch::Work);
        assert_eq!(decoded.results[0].candidates[0].work_title, candidate.work_title);
        assert_eq!(decoded.results[0].candidates[0].classifications, candidate.classifications);
        assert!(decoded.results[0].candidates_truncated);

        let request =
            EditionIdentityRequest { queries: vec![EditionIdentityQuery { query_id: "book-1".to_owned(), title: "Work title".to_owned(), authors: vec!["Alex Smith".to_owned()], publishers: Vec::new(), publication_year: Some(2004) }] };
        let decoded = decode_v2_edition_identity_request(&encode_v2_edition_identity_request(&request).unwrap(), MAX_REQUEST_BYTES).unwrap();
        assert_eq!(decoded.queries[0].title, "Work title");
    }
}
