pub use publication_model::{
    AgeRange, AggregateRating, Audience, Collection, CollectionKind, Identifier, InteractionStatistic, LanguageTag, LocalizedText, PopularityRank, PublicationDate, PublicationImage, PublicationProperty, PublicationSubject,
    PublicationTitle, Scheme, Scope,
};
use serde::{Deserialize, Serialize};

macro_rules! dual_serde {
    ($type:ty, $human:ident, $binary:ident) => {
        impl Serialize for $type {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                if serializer.is_human_readable() {
                    $human::serialize(self, serializer)
                } else {
                    $binary::serialize(self, serializer)
                }
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                if deserializer.is_human_readable() {
                    $human::deserialize(deserializer)
                } else {
                    $binary::deserialize(deserializer)
                }
            }
        }
    };
}

const MAX_CATALOG_ID_BYTES: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CatalogSourceId(String);

impl CatalogSourceId {
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty() || value.len() > MAX_CATALOG_ID_BYTES || !value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-') {
            return Err("catalog source id must contain only lowercase ASCII letters, digits, and hyphens".to_owned());
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for CatalogSourceId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogCapability {
    Browse,
    Search,
    Facets,
    Import,
    ExternalFulfillment,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogAuthentication {
    None,
    External { action_label: String, url: String },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogAuthentication", tag = "kind", rename_all = "snake_case")]
enum CatalogAuthenticationHuman {
    None,
    External { action_label: String, url: String },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogAuthentication")]
enum CatalogAuthenticationBinary {
    None,
    External { action_label: String, url: String },
}

dual_serde!(CatalogAuthentication, CatalogAuthenticationHuman, CatalogAuthenticationBinary);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSource {
    pub id: CatalogSourceId,
    pub title: String,
    pub description: String,
    pub languages: Vec<String>,
    pub authentication: CatalogAuthentication,
    pub capabilities: Vec<CatalogCapability>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogLinkTarget {
    Page { locator: String },
    External { url: String },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogLinkTarget", tag = "kind", rename_all = "snake_case")]
enum CatalogLinkTargetHuman {
    Page { locator: String },
    External { url: String },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogLinkTarget")]
enum CatalogLinkTargetBinary {
    Page { locator: String },
    External { url: String },
}

dual_serde!(CatalogLinkTarget, CatalogLinkTargetHuman, CatalogLinkTargetBinary);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogLink {
    pub title: String,
    pub target: CatalogLinkTarget,
    #[serde(default)]
    pub relations: Vec<String>,
    pub number_of_items: Option<u32>,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSearchField {
    Query,
    Title,
    Author,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogSearchDefinition {
    OpdsUriTemplate { base_url: String, template: String },
    OpdsOpenSearchDescription { url: String },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogSearchDefinition", tag = "kind", rename_all = "snake_case")]
enum CatalogSearchDefinitionHuman {
    OpdsUriTemplate { base_url: String, template: String },
    OpdsOpenSearchDescription { url: String },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogSearchDefinition")]
enum CatalogSearchDefinitionBinary {
    OpdsUriTemplate { base_url: String, template: String },
    OpdsOpenSearchDescription { url: String },
}

dual_serde!(CatalogSearchDefinition, CatalogSearchDefinitionHuman, CatalogSearchDefinitionBinary);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSearch {
    pub definition: CatalogSearchDefinition,
    pub fields: Vec<CatalogSearchField>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSearchQuery {
    pub query: String,
    pub title: Option<String>,
    pub author: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPagination {
    pub first: Option<String>,
    pub previous: Option<String>,
    pub next: Option<String>,
    pub last: Option<String>,
    pub number_of_items: Option<u32>,
    pub items_per_page: Option<u32>,
    pub current_page: Option<u32>,
}

/// A contiguous range within one fully filtered and sorted catalogue result
/// set. Provider page boundaries are deliberately absent from this contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogResultRange {
    pub offset: u32,
    pub limit: u32,
}

impl CatalogResultRange {
    pub const DEFAULT_LIMIT: u32 = 100;

    pub const fn new(offset: u32, limit: u32) -> Self {
        Self { offset, limit }
    }
}

impl Default for CatalogResultRange {
    fn default() -> Self {
        Self { offset: 0, limit: Self::DEFAULT_LIMIT }
    }
}

/// Metadata describing the returned slice of a logical catalogue result set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogResultRangeResponse {
    pub offset: u32,
    pub returned_items: u32,
    pub total_items: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogBookFormat {
    Epub,
    Pdf,
    Mobi,
    M4b,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogFreshness {
    Current,
    Stale,
}

impl CatalogBookFormat {
    pub const ALL: [Self; 4] = [Self::Epub, Self::Pdf, Self::Mobi, Self::M4b];

    pub fn label(self) -> &'static str {
        match self {
            Self::Epub => "EPUB",
            Self::Pdf => "PDF",
            Self::Mobi => "MOBI",
            Self::M4b => "M4B",
        }
    }

    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Epub => "application/epub+zip",
            Self::Pdf => "application/pdf",
            Self::Mobi => "application/x-mobipocket-ebook",
            Self::M4b => "audio/mp4",
        }
    }

    pub const fn file_extension(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Mobi => "mobi",
            Self::M4b => "m4b",
        }
    }
}

/// Stable ordering for aggregated catalogue subject results. The client
/// applies it to the synchronized locator collection before selecting the UI
/// range. It is deliberately separate from local-library ordering, whose
/// "recently read" state has no catalogue equivalent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSubjectSort {
    #[default]
    Popularity,
    Title,
}

impl CatalogSubjectSort {
    pub const ALL: [Self; 2] = [Self::Popularity, Self::Title];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Popularity => "Popularity",
            Self::Title => "Title",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPrice {
    pub currency: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CatalogPublicationIdentity {
    pub edition_id: String,
    pub work_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSubjectBrowseRequest {
    /// Opaque collection locator returned by `navigation`. It never contains a
    /// result offset. `None` opens the subject-system roots.
    pub locator: Option<String>,
    pub supported_languages: Vec<String>,
    /// Empty includes every catalogue source.
    #[serde(default)]
    pub source_ids: Vec<CatalogSourceId>,
    /// Case- and diacritic-insensitive title, author, or table-of-contents query.
    #[serde(default)]
    pub query: String,
    /// Ordering is applied across the logical collection before the requested
    /// result range is selected.
    #[serde(default)]
    pub sort: CatalogSubjectSort,
    /// Empty includes every importable format.
    #[serde(default)]
    pub book_formats: Vec<CatalogBookFormat>,
    /// Range within the result after search, filters, and sorting are applied.
    #[serde(default)]
    pub range: CatalogResultRange,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSourcedPublication {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublication,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSubjectBrowseEntryPayload {
    pub entry_id: u64,
    pub publication: CatalogSourcedPublication,
}

/// Resolves immutable publication cards after a client has received their
/// authoritative order from a subject browse request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSubjectPublicationPayloadRequest {
    pub epoch: String,
    pub entry_ids: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSubjectPublicationPayloadResponse {
    pub epoch: String,
    pub payloads: Vec<CatalogSubjectBrowseEntryPayload>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogBookFormatFacetValue {
    pub value: CatalogBookFormat,
    pub number_of_items: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogLanguageFacetValue {
    pub value: String,
    pub number_of_items: u32,
}

/// Operations and facet values supported at one catalogue browse location.
/// Empty vectors mean that the corresponding control is not available there.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSubjectBrowseCapabilities {
    #[serde(default)]
    pub sorts: Vec<CatalogSubjectSort>,
    #[serde(default)]
    pub book_formats: Vec<CatalogBookFormatFacetValue>,
    #[serde(default)]
    pub languages: Vec<CatalogLanguageFacetValue>,
}

/// Authoritative subject result order and controls for one requested range.
/// Native clients resolve immutable publication cards by `entry_ids`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSubjectBrowseIndexResponse {
    pub epoch: String,
    pub navigation: Vec<CatalogLink>,
    pub entry_ids: Vec<u64>,
    pub range: CatalogResultRangeResponse,
    #[serde(default)]
    pub capabilities: CatalogSubjectBrowseCapabilities,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogSubjectBrowseResponse {
    pub epoch: String,
    pub navigation: Vec<CatalogLink>,
    pub publications: Vec<CatalogSourcedPublication>,
    pub range: CatalogResultRangeResponse,
    #[serde(default)]
    pub capabilities: CatalogSubjectBrowseCapabilities,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogImportRequest {
    pub source_id: CatalogSourceId,
    pub catalog_locator: String,
    pub publication: CatalogPublicationIdentity,
    pub download_url: String,
    pub book_format: CatalogBookFormat,
}

impl CatalogImportRequest {
    /// Produces a portable filename without trusting a provider-controlled URL
    /// path. Platform adapters may still reject formats they cannot import.
    pub fn suggested_file_name(&self) -> String {
        let stem = self.publication.edition_id.chars().map(|character| if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') { character } else { '-' }).collect::<String>();
        let stem = stem.trim_matches('-');
        format!("{}.{}", if stem.is_empty() { "catalog-book" } else { stem }, self.book_format.file_extension())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogExternalActionKind {
    Open,
    Preview,
    Buy,
    Borrow,
    Subscribe,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogPublicationAction {
    Import { request: CatalogImportRequest },
    External { purpose: CatalogExternalActionKind, url: String, price: Option<CatalogPrice>, available_formats: Vec<CatalogBookFormat> },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogPublicationAction", tag = "kind", rename_all = "snake_case")]
enum CatalogPublicationActionHuman {
    Import { request: CatalogImportRequest },
    External { purpose: CatalogExternalActionKind, url: String, price: Option<CatalogPrice>, available_formats: Vec<CatalogBookFormat> },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogPublicationAction")]
enum CatalogPublicationActionBinary {
    Import { request: CatalogImportRequest },
    External { purpose: CatalogExternalActionKind, url: String, price: Option<CatalogPrice>, available_formats: Vec<CatalogBookFormat> },
}

dual_serde!(CatalogPublicationAction, CatalogPublicationActionHuman, CatalogPublicationActionBinary);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogDescription {
    PlainText { text: String },
    RichHtml { html: String },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogDescription", tag = "format", rename_all = "snake_case")]
enum CatalogDescriptionHuman {
    PlainText { text: String },
    RichHtml { html: String },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "CatalogDescription")]
enum CatalogDescriptionBinary {
    PlainText { text: String },
    RichHtml { html: String },
}

dual_serde!(CatalogDescription, CatalogDescriptionHuman, CatalogDescriptionBinary);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogAuthorCredit {
    pub name: String,
    #[serde(default)]
    pub name_localizations: Vec<LocalizedText>,
    pub sort_name: Option<String>,
    #[serde(default)]
    pub sort_name_localizations: Vec<LocalizedText>,
    pub identifiers: Vec<String>,
    pub provider_link: Option<CatalogLinkTarget>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogPublicationMetadata {
    pub title_localizations: Vec<LocalizedText>,
    pub sort_name: Option<String>,
    pub sort_name_localizations: Vec<LocalizedText>,
    pub published: Option<String>,
    pub modified: Option<String>,
    pub dates: Vec<PublicationDate>,
    pub publishers: Vec<CatalogPublisherCredit>,
    pub identifiers: Vec<Identifier>,
    pub subjects: Vec<PublicationSubject>,
    pub subtitle: Option<String>,
    pub subtitle_localizations: Vec<LocalizedText>,
    pub alternate_titles: Vec<PublicationTitle>,
    pub collections: Vec<Collection>,
    pub contributors: Vec<CatalogContributorCredit>,
    pub rights: Vec<String>,
    pub sources: Vec<String>,
    pub types: Vec<String>,
    pub formats: Vec<String>,
    pub relations: Vec<String>,
    pub coverage: Vec<String>,
    pub properties: Vec<PublicationProperty>,
    pub audiences: Vec<Audience>,
    pub age_ranges: Vec<AgeRange>,
    pub images: Vec<PublicationImage>,
    pub ratings: Vec<AggregateRating>,
    pub interactions: Vec<InteractionStatistic>,
    pub popularity_ranks: Vec<PopularityRank>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPublisherCredit {
    pub name: String,
    #[serde(default)]
    pub name_localizations: Vec<LocalizedText>,
    pub sort_name: Option<String>,
    #[serde(default)]
    pub sort_name_localizations: Vec<LocalizedText>,
    pub identifiers: Vec<String>,
    pub provider_link: Option<CatalogLinkTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogContributorCredit {
    pub name: String,
    #[serde(default)]
    pub name_localizations: Vec<LocalizedText>,
    pub sort_name: Option<String>,
    #[serde(default)]
    pub sort_name_localizations: Vec<LocalizedText>,
    pub identifiers: Vec<String>,
    pub provider_link: Option<CatalogLinkTarget>,
    /// Exact OPDS role values; unknown roles remain available for later mapping.
    pub roles: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPublication {
    pub identity: CatalogPublicationIdentity,
    pub title: String,
    pub authors: Vec<CatalogAuthorCredit>,
    /// Canonical BCP-47 language tags advertised for this publication.
    pub languages: Vec<LanguageTag>,
    pub description: Option<CatalogDescription>,
    pub image_url: Option<String>,
    #[serde(default)]
    pub metadata: CatalogPublicationMetadata,
    pub actions: Vec<CatalogPublicationAction>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogGroup {
    pub title: String,
    pub page_locator: Option<String>,
    pub number_of_items: Option<u32>,
    pub navigation: Vec<CatalogLink>,
    pub publications: Vec<CatalogPublication>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogFacet {
    pub title: String,
    pub links: Vec<CatalogLink>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPage {
    pub source_id: CatalogSourceId,
    pub freshness: CatalogFreshness,
    pub title: String,
    pub locator: String,
    pub navigation: Vec<CatalogLink>,
    pub publications: Vec<CatalogPublication>,
    pub groups: Vec<CatalogGroup>,
    pub facets: Vec<CatalogFacet>,
    pub pagination: CatalogPagination,
    pub search: Option<CatalogSearch>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_targets_cannot_mix_page_and_external_destinations() {
        let page = CatalogLinkTarget::Page { locator: "https://catalog.example/new".to_owned() };
        let external = CatalogLinkTarget::External { url: "https://publisher.example/book".to_owned() };
        assert_eq!(serde_json::to_value(page).unwrap(), serde_json::json!({"kind":"page","locator":"https://catalog.example/new"}));
        assert_eq!(serde_json::to_value(external).unwrap(), serde_json::json!({"kind":"external","url":"https://publisher.example/book"}));
    }

    #[test]
    fn source_ids_reject_protocol_and_path_data() {
        assert!(CatalogSourceId::parse("opds-example").is_ok());
        assert!(CatalogSourceId::parse("https://example.com").is_err());
        assert!(CatalogSourceId::parse("../catalog").is_err());
    }

    #[test]
    fn legacy_subject_browse_requests_use_default_sort_formats_and_range() {
        let request: CatalogSubjectBrowseRequest = serde_json::from_value(serde_json::json!({
            "locator": null,
            "supported_languages": ["en"],
            "source_ids": [],
            "query": "history"
        }))
        .unwrap();

        assert_eq!(request.sort, CatalogSubjectSort::Popularity);
        assert!(request.book_formats.is_empty());
        assert_eq!(request.range, CatalogResultRange::default());
    }

    #[test]
    fn legacy_subject_browse_responses_have_empty_capabilities() {
        let response: CatalogSubjectBrowseResponse = serde_json::from_value(serde_json::json!({
            "epoch": "catalogue-1",
            "navigation": [],
            "publications": [],
            "range": {
                "offset": 0,
                "returned_items": 0,
                "total_items": 0
            }
        }))
        .unwrap();

        assert_eq!(response.capabilities, CatalogSubjectBrowseCapabilities::default());
    }

    #[test]
    fn catalog_import_filenames_are_portable_and_format_aware() {
        let request = CatalogImportRequest {
            source_id: CatalogSourceId::parse("opds-test").unwrap(),
            catalog_locator: "https://catalog.example/root".to_owned(),
            publication: CatalogPublicationIdentity { edition_id: "urn:book/edition 1".to_owned(), work_id: None },
            download_url: "https://catalog.example/download".to_owned(),
            book_format: CatalogBookFormat::Epub,
        };

        assert_eq!(request.suggested_file_name(), "urn-book-edition-1.epub");
        assert_eq!(request.book_format.media_type(), "application/epub+zip");
    }
}
