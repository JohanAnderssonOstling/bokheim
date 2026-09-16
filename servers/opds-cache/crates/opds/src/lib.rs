use atom_syndication::{Entry as AtomEntry, Feed, Link as AtomLink, TextType};
use futures_util::StreamExt;
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};
use reqwest::{Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

mod litteraturbanken;
mod registry;

pub(crate) use outbound;
pub use outbound::{send_get as send_outbound_get, OutboundHttpError, OutboundNetworkPolicy};
pub use publication_model::{
    AgeRange as OpdsAgeRange, AggregateRating as OpdsAggregateRating, Audience as OpdsAudience, Collection as OpdsPublicationCollection, CollectionKind, InteractionStatistic as OpdsInteractionStatistic, LanguageTag as OpdsLanguageTag,
    LocalizedText as OpdsLocalizedText, PopularityRank as OpdsPopularityRank, PublicationDate as OpdsPublicationDate, PublicationImage as OpdsPublicationImage, PublicationProperty as OpdsPublicationProperty,
    PublicationSubject as OpdsPublicationSubject, PublicationTitle as OpdsPublicationTitle,
};
pub use registry::{CatalogRegistry, CatalogSource, CATALOG_REGISTRY_SCHEMA_VERSION};

const MAX_CATALOG_BYTES: u64 = 24 * 1024 * 1024;
const MAX_REGISTRY_BYTES: u64 = 2 * 1024 * 1024;
// Bounds untrusted third-party catalog downloads before they enter a library;
// synchronized storage itself is governed by the user's account quota.
const MAX_OPDS_DOWNLOAD_BYTES: u64 = 1_000_000_000;
const NETWORK_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const EPUB_RANGE_TAIL_BYTES: u64 = 65_557;
const MAX_EPUB_RANGE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_EPUB_CENTRAL_DIRECTORY_BYTES: u64 = 8 * 1024 * 1024;
const MAX_EPUB_CONTAINER_BYTES: u64 = 256 * 1024;
const MAX_EPUB_PACKAGE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_EPUB_NAVIGATION_BYTES: u64 = 4 * 1024 * 1024;
const MAX_EPUB_COVER_BYTES: u64 = 24 * 1024 * 1024;
const MAX_RETAINED_METADATA_PROPERTIES: usize = 64;
const MAX_RETAINED_METADATA_PROPERTY_BYTES: usize = 16 * 1024;
const EPUB_MEDIA_TYPE: &str = "application/epub+zip";
const PDF_MEDIA_TYPE: &str = "application/pdf";
const MOBI_MEDIA_TYPE: &str = "application/x-mobipocket-ebook";
const M4B_MEDIA_TYPE: &str = "audio/mp4";
const OPDS_2_MEDIA_TYPE: &str = "application/opds+json";

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct OpdsEndpointId(String);

impl OpdsEndpointId {
    pub fn parse(value: &str) -> Result<Self, OpdsError> {
        if value.is_empty() || value.len() > 80 || !value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-') {
            return Err(OpdsError::InvalidEndpoint("catalog id must contain only lowercase ASCII letters, digits, and hyphens".to_owned()));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for OpdsEndpointId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsEndpoint {
    pub id: OpdsEndpointId,
    pub title: String,
    pub description: String,
    pub languages: Vec<String>,
    pub url: String,
}

pub fn built_in_opds_endpoints() -> Vec<OpdsEndpoint> {
    vec![
        open_endpoint("project-gutenberg", "Project Gutenberg", "A large multilingual collection of free ebooks, with browsing and catalog search.", &["Multilingual"], "https://www.gutenberg.org/ebooks/search.opds/"),
        open_endpoint("unglue-it", "Unglue.it", "Open and Creative Commons ebooks collected by the Free Ebook Foundation.", &["Multilingual"], "https://unglue.it/api/opds/"),
        open_endpoint("gallica", "Gallica", "Public-domain publications from the Bibliothèque nationale de France.", &["French"], "https://gallica.bnf.fr/opds"),
        open_endpoint(
            "litteraturbanken",
            "Litteraturbanken",
            "Swedish literature from Litteraturbanken, available as free EPUB downloads.",
            &["Swedish", "English", "German", "French", "Latin", "Sámi languages"],
            ::litteraturbanken::CATALOG_URL,
        ),
    ]
}

fn open_endpoint(id: &str, title: &str, description: &str, languages: &[&str], url: &str) -> OpdsEndpoint {
    OpdsEndpoint {
        id: OpdsEndpointId::parse(id).expect("bundled catalog id is valid"),
        title: title.to_owned(),
        description: description.to_owned(),
        languages: languages.iter().map(|language| (*language).to_owned()).collect(),
        url: url.to_owned(),
    }
}

pub fn endpoints_from_registry(registry: CatalogRegistry) -> Result<Vec<OpdsEndpoint>, OpdsError> {
    if registry.schema_version != CATALOG_REGISTRY_SCHEMA_VERSION {
        return Err(OpdsError::InvalidEndpoint(format!("unsupported registry schema {}", registry.schema_version)));
    }
    if registry.revision == 0 {
        return Err(OpdsError::InvalidEndpoint("registry revision must be greater than zero".to_owned()));
    }
    let mut ids = std::collections::HashSet::new();
    registry.catalogs.into_iter().map(|source| endpoint_from_source(source, &mut ids)).collect()
}

pub fn validate_curated_registry(registry: &CatalogRegistry) -> Result<(), OpdsError> {
    let endpoints = endpoints_from_registry(registry.clone())?;
    if endpoints.is_empty() {
        return Err(OpdsError::InvalidEndpoint("the curated OPDS registry must contain at least one catalog".to_owned()));
    }
    Ok(())
}

pub fn registry_from_endpoints(revision: u64, endpoints: &[OpdsEndpoint]) -> CatalogRegistry {
    CatalogRegistry {
        schema_version: CATALOG_REGISTRY_SCHEMA_VERSION,
        revision,
        catalogs: endpoints
            .iter()
            .map(|endpoint| CatalogSource { id: endpoint.id.as_str().to_owned(), title: endpoint.title.clone(), description: endpoint.description.clone(), languages: endpoint.languages.clone(), url: endpoint.url.clone() })
            .collect(),
    }
}

pub fn load_registry_cache(path: &Path) -> Result<Option<(CatalogRegistry, Vec<OpdsEndpoint>)>, OpdsError> {
    let Some(value) = load_registry_cache_value(path)? else { return Ok(None) };
    let registry: CatalogRegistry = serde_json::from_value(value).map_err(|error| OpdsError::InvalidEndpoint(error.to_string()))?;
    let endpoints = endpoints_from_registry(registry.clone())?;
    Ok(Some((registry, endpoints)))
}

fn load_registry_cache_value(path: &Path) -> Result<Option<Value>, OpdsError> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if bytes.len() as u64 > MAX_REGISTRY_BYTES {
        return Err(OpdsError::ResponseTooLarge { limit: MAX_REGISTRY_BYTES });
    }
    serde_json::from_slice(&bytes).map(Some).map_err(|error| OpdsError::InvalidEndpoint(error.to_string()))
}

pub fn save_registry_cache(path: &Path, registry: &CatalogRegistry) -> Result<(), OpdsError> {
    let bytes = serde_json::to_vec(registry).map_err(|error| OpdsError::InvalidEndpoint(error.to_string()))?;
    if bytes.len() as u64 > MAX_REGISTRY_BYTES {
        return Err(OpdsError::ResponseTooLarge { limit: MAX_REGISTRY_BYTES });
    }
    let parent = path.parent().ok_or_else(|| OpdsError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, "OPDS registry cache path has no parent")))?;
    std::fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".opds-catalogs-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), std::io::Error> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(OpdsError::Io)
}

fn endpoint_from_source(source: CatalogSource, ids: &mut std::collections::HashSet<String>) -> Result<OpdsEndpoint, OpdsError> {
    let id = OpdsEndpointId::parse(&source.id)?;
    if !ids.insert(source.id) {
        return Err(OpdsError::InvalidEndpoint(format!("duplicate catalog id {}", id.as_str())));
    }
    let title = bounded_endpoint_text(source.title, "title", 160)?;
    let description = bounded_endpoint_text(source.description, "description", 1000)?;
    if source.languages.is_empty() || source.languages.len() > 32 {
        return Err(OpdsError::InvalidEndpoint(format!("catalog {} must have between 1 and 32 languages", id.as_str())));
    }
    let languages = source.languages.into_iter().map(|language| bounded_endpoint_text(language, "language", 80)).collect::<Result<Vec<_>, _>>()?;
    let url = parse_http_url(&source.url)?.to_string();
    if !url.starts_with("https://") {
        return Err(OpdsError::InvalidEndpoint(format!("curated catalog {} must use HTTPS", id.as_str())));
    }
    Ok(OpdsEndpoint { id, title, description, languages, url })
}

fn bounded_endpoint_text(value: String, field: &str, maximum: usize) -> Result<String, OpdsError> {
    let value = value.trim();
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        Err(OpdsError::InvalidEndpoint(format!("catalog {field} must be non-empty bounded text without control characters")))
    } else {
        Ok(value.to_owned())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpdsVersion {
    V1_2,
    V2_0,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpdsCatalogFreshness {
    #[default]
    Current,
    Stale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpdsAcquisitionKind {
    OpenAccess,
    Borrow,
    Buy,
    Preview,
    Subscribe,
    Generic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpdsLinkTarget {
    Catalog,
    Web,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsCatalogLink {
    pub title: String,
    pub url: String,
    pub target: OpdsLinkTarget,
    /// Exact link-relation values advertised by the provider.
    #[serde(default)]
    pub relations: Vec<String>,
    pub number_of_items: Option<u32>,
    pub active: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsPagination {
    pub first: Option<String>,
    pub previous: Option<String>,
    pub next: Option<String>,
    pub last: Option<String>,
    pub number_of_items: Option<u32>,
    pub items_per_page: Option<u32>,
    pub current_page: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpdsBookFormat {
    Epub,
    Pdf,
    Mobi,
    M4b,
}

impl OpdsBookFormat {
    fn media_type(self) -> &'static str {
        match self {
            Self::Epub => EPUB_MEDIA_TYPE,
            Self::Pdf => PDF_MEDIA_TYPE,
            Self::Mobi => MOBI_MEDIA_TYPE,
            Self::M4b => M4B_MEDIA_TYPE,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Mobi => "mobi",
            Self::M4b => "m4b",
        }
    }

    fn accepts_media_type(self, value: &str) -> bool {
        book_format_from_media_type(value) == Some(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsPrice {
    pub currency: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsDirectAcquisition {
    pub url: String,
    pub kind: OpdsAcquisitionKind,
    pub book_format: OpdsBookFormat,
    pub suggested_file_name: String,
    pub price: Option<OpdsPrice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsIndirectAcquisition {
    pub url: String,
    pub kind: OpdsAcquisitionKind,
    pub media_type: Option<String>,
    pub available_formats: Vec<OpdsBookFormat>,
    pub price: Option<OpdsPrice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsUnsupportedAcquisition {
    pub url: String,
    pub kind: OpdsAcquisitionKind,
    pub media_type: Option<String>,
    pub price: Option<OpdsPrice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "delivery", rename_all = "snake_case")]
pub enum OpdsAcquisition {
    Direct(OpdsDirectAcquisition),
    Indirect(OpdsIndirectAcquisition),
    Unsupported(OpdsUnsupportedAcquisition),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsAuthorCredit {
    pub name: String,
    #[serde(default)]
    pub name_localizations: Vec<OpdsLocalizedText>,
    pub sort_name: Option<String>,
    #[serde(default)]
    pub sort_name_localizations: Vec<OpdsLocalizedText>,
    pub identifiers: Vec<String>,
    pub provider_link: Option<OpdsAuthorLink>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsAuthorLink {
    pub url: String,
    pub target: OpdsLinkTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsPublication {
    pub id: String,
    pub work_id: Option<String>,
    pub title: String,
    pub authors: Vec<OpdsAuthorCredit>,
    pub languages: Vec<String>,
    pub description: Option<OpdsDescription>,
    pub image_url: Option<String>,
    #[serde(default)]
    pub metadata: OpdsPublicationMetadata,
    pub preferred_download: Option<OpdsDirectAcquisition>,
    pub acquisitions: Vec<OpdsAcquisition>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpdsIdentifierScope {
    Publication,
    CatalogEntry,
    Edition,
    Work,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsPublicationIdentifier {
    pub value: String,
    pub scheme: Option<String>,
    pub source: String,
    pub scope: OpdsIdentifierScope,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OpdsPublicationMetadata {
    pub title_localizations: Vec<OpdsLocalizedText>,
    pub sort_name: Option<String>,
    pub sort_name_localizations: Vec<OpdsLocalizedText>,
    pub published: Option<String>,
    pub modified: Option<String>,
    pub dates: Vec<OpdsPublicationDate>,
    pub publishers: Vec<OpdsAgentCredit>,
    pub identifiers: Vec<OpdsPublicationIdentifier>,
    pub subjects: Vec<OpdsPublicationSubject>,
    pub subtitle: Option<String>,
    pub subtitle_localizations: Vec<OpdsLocalizedText>,
    pub alternate_titles: Vec<OpdsPublicationTitle>,
    pub collections: Vec<OpdsPublicationCollection>,
    pub contributors: Vec<OpdsContributorCredit>,
    pub rights: Vec<String>,
    pub sources: Vec<String>,
    pub types: Vec<String>,
    pub formats: Vec<String>,
    pub relations: Vec<String>,
    pub coverage: Vec<String>,
    pub audiences: Vec<OpdsAudience>,
    pub age_ranges: Vec<OpdsAgeRange>,
    pub images: Vec<OpdsPublicationImage>,
    pub ratings: Vec<OpdsAggregateRating>,
    pub interactions: Vec<OpdsInteractionStatistic>,
    pub popularity_ranks: Vec<OpdsPopularityRank>,
    /// Raw provider metadata retained losslessly when reasonably sized.
    pub properties: Vec<OpdsPublicationProperty>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsAgentCredit {
    pub name: String,
    #[serde(default)]
    pub name_localizations: Vec<OpdsLocalizedText>,
    pub sort_name: Option<String>,
    #[serde(default)]
    pub sort_name_localizations: Vec<OpdsLocalizedText>,
    pub identifiers: Vec<String>,
    pub provider_link: Option<OpdsAuthorLink>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsContributorCredit {
    pub name: String,
    #[serde(default)]
    pub name_localizations: Vec<OpdsLocalizedText>,
    pub sort_name: Option<String>,
    #[serde(default)]
    pub sort_name_localizations: Vec<OpdsLocalizedText>,
    pub identifiers: Vec<String>,
    pub provider_link: Option<OpdsAuthorLink>,
    pub roles: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum OpdsDescription {
    PlainText { text: String },
    Html { markup: String },
    Xhtml { markup: String },
}

impl OpdsDescription {
    pub fn markup(&self) -> Option<&str> {
        match self {
            Self::PlainText { .. } => None,
            Self::Html { markup, .. } | Self::Xhtml { markup, .. } => Some(markup),
        }
    }
}

impl OpdsPublication {
    pub fn identity(&self) -> OpdsPublicationIdentity {
        OpdsPublicationIdentity { publication_id: self.id.clone(), work_id: self.work_id.clone() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpdsPublicationIdentity {
    pub publication_id: String,
    pub work_id: Option<String>,
}

impl OpdsPublicationIdentity {
    pub fn validate(&self) -> Result<(), OpdsError> {
        if !valid_catalog_identity_text(&self.publication_id) || self.work_id.as_deref().is_some_and(|value| !valid_catalog_identity_text(value)) {
            return Err(OpdsError::InvalidCatalog("catalog publication identity must be non-empty, bounded text without control characters".to_owned()));
        }
        Ok(())
    }
}

fn valid_catalog_identity_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 2048 && !value.chars().any(char::is_control)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpdsOwnershipStatus {
    NotOwned,
    OtherEditionOwned,
    ExactEditionOwned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsPublicationOwnership {
    pub publication_id: String,
    pub status: OpdsOwnershipStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsGroup {
    pub title: String,
    pub self_url: Option<String>,
    pub number_of_items: Option<u32>,
    pub navigation: Vec<OpdsCatalogLink>,
    pub publications: Vec<OpdsPublication>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsFacet {
    pub title: String,
    pub links: Vec<OpdsCatalogLink>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsCatalog {
    pub version: OpdsVersion,
    #[serde(default)]
    pub freshness: OpdsCatalogFreshness,
    pub title: String,
    pub source_url: String,
    pub self_url: Option<String>,
    pub start_url: Option<String>,
    pub navigation: Vec<OpdsCatalogLink>,
    pub publications: Vec<OpdsPublication>,
    pub groups: Vec<OpdsGroup>,
    pub facets: Vec<OpdsFacet>,
    pub pagination: OpdsPagination,
}

#[derive(Debug)]
pub enum OpdsError {
    InvalidEndpoint(String),
    InvalidUrl(String),
    Network(reqwest::Error),
    TimedOut,
    HttpStatus { status: reqwest::StatusCode, retry_after: Option<Duration> },
    ResponseTooLarge { limit: u64 },
    RangeUnsupported,
    UnsupportedCatalogFormat,
    InvalidCatalog(String),
    NonFreeAcquisition,
    UnsupportedBookResponse(Option<String>),
    Io(std::io::Error),
}

impl fmt::Display for OpdsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEndpoint(error) => write!(formatter, "invalid OPDS endpoint: {error}"),
            Self::InvalidUrl(error) => write!(formatter, "invalid OPDS URL: {error}"),
            Self::Network(error) => write!(formatter, "OPDS network request failed: {error}"),
            Self::TimedOut => formatter.write_str("OPDS server did not respond before the timeout"),
            Self::HttpStatus { status, retry_after } => {
                write!(formatter, "OPDS server returned HTTP {status}")?;
                if let Some(retry_after) = retry_after {
                    write!(formatter, " (retry after {} seconds)", retry_after.as_secs())?;
                }
                Ok(())
            }
            Self::ResponseTooLarge { limit } => write!(formatter, "OPDS response exceeds the {limit}-byte limit"),
            Self::RangeUnsupported => formatter.write_str("OPDS acquisition origin does not support reliable byte ranges"),
            Self::UnsupportedCatalogFormat => formatter.write_str("response is neither an OPDS 1.2 Atom feed nor an OPDS 2.0 JSON feed"),
            Self::InvalidCatalog(error) => write!(formatter, "invalid OPDS catalog: {error}"),
            Self::NonFreeAcquisition => formatter.write_str("only unpriced direct open-access or generic EPUB/PDF acquisitions can be imported"),
            Self::UnsupportedBookResponse(media_type) => write!(formatter, "OPDS acquisition returned unsupported content type {}", media_type.as_deref().unwrap_or("unknown")),
            Self::Io(error) => write!(formatter, "OPDS file operation failed: {error}"),
        }
    }
}

impl std::error::Error for OpdsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Network(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl OpdsError {
    pub fn http_status(&self) -> Option<reqwest::StatusCode> {
        match self {
            Self::HttpStatus { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::HttpStatus { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

impl From<reqwest::Error> for OpdsError {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error)
    }
}

impl From<std::io::Error> for OpdsError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug)]
pub struct OpdsFetchProfile {
    pub requested_url: String,
    pub final_url: Option<String>,
    pub http_fetches: usize,
    pub response_bytes: usize,
    pub response_headers: Duration,
    pub body_download: Duration,
    pub discovery: Duration,
    pub parsing: Duration,
    pub total: Duration,
}

impl OpdsFetchProfile {
    fn new(requested_url: String) -> Self {
        Self { requested_url, final_url: None, http_fetches: 0, response_bytes: 0, response_headers: Duration::ZERO, body_download: Duration::ZERO, discovery: Duration::ZERO, parsing: Duration::ZERO, total: Duration::ZERO }
    }
}

#[derive(Clone)]
pub struct OpdsClient {
    outbound_policy: OutboundNetworkPolicy,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpdsHttpValidators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum OpdsConditionalCatalog {
    NotModified,
    Modified { catalog: OpdsCatalog, validators: OpdsHttpValidators },
}

impl OpdsClient {
    pub fn new() -> Result<Self, OpdsError> {
        Ok(Self { outbound_policy: OutboundNetworkPolicy::AllowPrivate })
    }

    /// Creates the client used by Internet-facing services. Every DNS result
    /// and redirect hop must remain on publicly routed address space.
    pub fn new_public_only() -> Result<Self, OpdsError> {
        Ok(Self { outbound_policy: OutboundNetworkPolicy::PublicOnly })
    }

    pub async fn fetch_catalog(&self, url: &str) -> Result<OpdsCatalog, OpdsError> {
        self.fetch_catalog_url(parse_http_url(url)?, MAX_CATALOG_BYTES).await
    }

    pub async fn fetch_catalog_fresh(&self, url: &str) -> Result<OpdsCatalog, OpdsError> {
        self.fetch_catalog(url).await
    }

    pub async fn fetch_catalog_conditional(&self, url: &str, validators: &OpdsHttpValidators) -> Result<OpdsConditionalCatalog, OpdsError> {
        self.fetch_catalog_url_conditional(parse_http_url(url)?, MAX_CATALOG_BYTES, validators).await
    }

    /// Performs a cold catalog fetch and returns timings for each expensive stage.
    /// This intentionally bypasses the catalog cache so diagnostics measure the
    /// upstream server, transfer, discovery, and parser rather than a cache hit.
    pub async fn profile_catalog_fetch(&self, url: &str) -> (Result<OpdsCatalog, OpdsError>, OpdsFetchProfile) {
        let started = Instant::now();
        let parsed_url = match parse_http_url(url) {
            Ok(url) => url,
            Err(error) => {
                let mut profile = OpdsFetchProfile::new(url.to_owned());
                profile.total = started.elapsed();
                return (Err(error), profile);
            }
        };
        let (result, mut profile) = self.fetch_catalog_url_profiled(parsed_url, MAX_CATALOG_BYTES).await;
        profile.total = started.elapsed();
        (result, profile)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn download_book(&self, acquisition: &OpdsDirectAcquisition, temporary_directory: &Path) -> Result<DownloadedOpdsBook, OpdsError> {
        self.download_book_with_limit(acquisition, temporary_directory, MAX_OPDS_DOWNLOAD_BYTES).await
    }

    /// Downloads an acquisition with a caller-specific bound no larger than
    /// the normal import limit. Catalogue crawlers use this to avoid acquiring
    /// unusually large books merely to inspect their package metadata.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn download_book_with_limit(&self, acquisition: &OpdsDirectAcquisition, temporary_directory: &Path, byte_limit: u64) -> Result<DownloadedOpdsBook, OpdsError> {
        if !is_importable_acquisition(acquisition) {
            return Err(OpdsError::NonFreeAcquisition);
        }
        let byte_limit = byte_limit.min(MAX_OPDS_DOWNLOAD_BYTES);
        let url = parse_http_url(&acquisition.url)?;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::ACCEPT, reqwest::header::HeaderValue::from_static(acquisition.book_format.media_type()));
        litteraturbanken::decorate_download_headers(&mut headers, &url);
        let response = self.send(url.clone(), headers).await?;
        let _ = validated_response_url(&response)?;
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(normalized_media_type);
        if media_type.as_deref().is_some_and(|value| !acquisition.book_format.accepts_media_type(value) && !matches!(value, "application/octet-stream" | "application/zip" | "binary/octet-stream")) {
            return Err(OpdsError::UnsupportedBookResponse(media_type));
        }
        if response.content_length().is_some_and(|length| length > byte_limit) {
            return Err(OpdsError::ResponseTooLarge { limit: byte_limit });
        }
        tokio::fs::create_dir_all(temporary_directory).await?;
        let path = temporary_directory.join(format!("opds-{}.{}", uuid::Uuid::new_v4(), acquisition.book_format.extension()));
        let mut guard = DownloadedOpdsBook { path, file_name: acquisition.suggested_file_name.clone(), transferred_bytes: 0, metadata_only: false };
        let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&guard.path).await?;
        let mut stream = response.bytes_stream();
        let mut downloaded = 0_u64;
        while let Some(chunk) = tokio::time::timeout(NETWORK_IDLE_TIMEOUT, stream.next()).await.map_err(|_| OpdsError::TimedOut)? {
            let chunk = chunk?;
            downloaded = downloaded.checked_add(chunk.len() as u64).ok_or(OpdsError::ResponseTooLarge { limit: byte_limit })?;
            if downloaded > byte_limit {
                return Err(OpdsError::ResponseTooLarge { limit: byte_limit });
            }
            file.write_all(&chunk).await?;
        }
        if downloaded == 0 {
            return Err(OpdsError::InvalidCatalog("book acquisition returned an empty response".to_owned()));
        }
        file.flush().await?;
        drop(file);
        guard.transferred_bytes = downloaded;
        Ok(guard)
    }

    /// Materializes a sparse EPUB containing only the ZIP directory, package
    /// metadata, navigation document, and (when requested) declared cover.
    /// Callers must fall back to [`Self::download_book_with_limit`] when the
    /// origin does not honor byte ranges or the archive uses unsupported ZIP64
    /// metadata. The returned file remains readable by the normal EPUB parser.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn download_epub_metadata_with_limit(&self, acquisition: &OpdsDirectAcquisition, temporary_directory: &Path, byte_limit: u64, include_cover: bool) -> Result<DownloadedOpdsBook, OpdsError> {
        if !is_importable_acquisition(acquisition) {
            return Err(OpdsError::NonFreeAcquisition);
        }
        if acquisition.book_format != OpdsBookFormat::Epub {
            return Err(OpdsError::InvalidCatalog("ranged package inspection is only available for EPUB acquisitions".to_owned()));
        }
        let byte_limit = byte_limit.min(MAX_OPDS_DOWNLOAD_BYTES);
        let requested_url = parse_http_url(&acquisition.url)?;
        let first = self.fetch_book_range(&requested_url, acquisition, format!("bytes=-{EPUB_RANGE_TAIL_BYTES}"), None, None, EPUB_RANGE_TAIL_BYTES).await?;
        if first.total_length == 0 || first.total_length > byte_limit {
            return Err(OpdsError::ResponseTooLarge { limit: byte_limit });
        }
        let (central_offset, central_size) = epub_central_directory_bounds(&first.bytes, first.start, first.total_length)?;
        if central_size > MAX_EPUB_CENTRAL_DIRECTORY_BYTES {
            return Err(OpdsError::ResponseTooLarge { limit: MAX_EPUB_CENTRAL_DIRECTORY_BYTES });
        }

        tokio::fs::create_dir_all(temporary_directory).await?;
        let path = temporary_directory.join(format!("opds-{}.epub", uuid::Uuid::new_v4()));
        let mut guard = DownloadedOpdsBook { path, file_name: acquisition.suggested_file_name.clone(), transferred_bytes: 0, metadata_only: true };
        let file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&guard.path).await?;
        file.set_len(first.total_length).await?;
        drop(file);
        write_sparse_range(&guard.path, first.start, &first.bytes).await?;

        let mut transferred_bytes = first.bytes.len() as u64;
        let central = self.fetch_book_range(&first.final_url, acquisition, inclusive_range_header(central_offset, central_size)?, first.validator.as_deref(), Some(first.total_length), central_size).await?;
        transferred_bytes = checked_range_budget(transferred_bytes, central.bytes.len() as u64)?;
        write_sparse_range(&guard.path, central.start, &central.bytes).await?;
        let entries = parse_epub_central_directory(&central.bytes, central_offset)?;

        let container_name = "META-INF/container.xml";
        transferred_bytes = self.materialize_epub_entry(&first.final_url, acquisition, first.validator.as_deref(), first.total_length, &guard.path, &entries, container_name, MAX_EPUB_CONTAINER_BYTES, transferred_bytes).await?;
        let container = read_epub_entry(&guard.path, container_name, MAX_EPUB_CONTAINER_BYTES)?;
        let package_name = epub_package_path(&container)?;
        transferred_bytes = self.materialize_epub_entry(&first.final_url, acquisition, first.validator.as_deref(), first.total_length, &guard.path, &entries, &package_name, MAX_EPUB_PACKAGE_BYTES, transferred_bytes).await?;
        let package = read_epub_entry(&guard.path, &package_name, MAX_EPUB_PACKAGE_BYTES)?;
        let related = epub_package_related_entries(&package_name, &package)?;
        for navigation in related.navigation {
            transferred_bytes = self.materialize_epub_entry(&first.final_url, acquisition, first.validator.as_deref(), first.total_length, &guard.path, &entries, &navigation, MAX_EPUB_NAVIGATION_BYTES, transferred_bytes).await?;
        }
        if include_cover {
            let cover = related.cover.ok_or_else(|| OpdsError::InvalidCatalog("EPUB has no directly declared cover suitable for ranged extraction".to_owned()))?;
            transferred_bytes = self.materialize_epub_entry(&first.final_url, acquisition, first.validator.as_deref(), first.total_length, &guard.path, &entries, &cover, MAX_EPUB_COVER_BYTES, transferred_bytes).await?;
        }
        guard.transferred_bytes = transferred_bytes;
        Ok(guard)
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn materialize_epub_entry(
        &self, url: &Url, acquisition: &OpdsDirectAcquisition, validator: Option<&str>, total_length: u64, path: &Path, entries: &HashMap<String, RemoteZipEntry>, name: &str, uncompressed_limit: u64, transferred_bytes: u64,
    ) -> Result<u64, OpdsError> {
        let entry = entries.get(name).ok_or_else(|| OpdsError::InvalidCatalog(format!("EPUB ZIP entry is missing: {name}")))?;
        if entry.encrypted {
            return Err(OpdsError::InvalidCatalog(format!("EPUB ZIP entry is encrypted: {name}")));
        }
        if entry.uncompressed_size > uncompressed_limit {
            return Err(OpdsError::ResponseTooLarge { limit: uncompressed_limit });
        }
        let header = self.fetch_book_range(url, acquisition, format!("bytes={}-{}", entry.header_start, entry.header_start.saturating_add(29)), validator, Some(total_length), 30).await?;
        if header.bytes.len() != 30 || header.bytes.get(..4) != Some(b"PK\x03\x04".as_slice()) {
            return Err(OpdsError::InvalidCatalog(format!("EPUB ZIP local header is invalid: {name}")));
        }
        let name_length = little_u16(&header.bytes, 26)? as u64;
        let extra_length = little_u16(&header.bytes, 28)? as u64;
        let range_size = 30_u64
            .checked_add(name_length)
            .and_then(|value| value.checked_add(extra_length))
            .and_then(|value| value.checked_add(entry.compressed_size))
            .ok_or_else(|| OpdsError::InvalidCatalog("EPUB ZIP entry range overflowed".to_owned()))?;
        if entry.header_start.checked_add(range_size).is_none_or(|end| end > total_length) {
            return Err(OpdsError::InvalidCatalog(format!("EPUB ZIP entry exceeds the archive: {name}")));
        }
        let transferred_bytes = checked_range_budget(transferred_bytes, header.bytes.len() as u64)?;
        let transferred_bytes = checked_range_budget(transferred_bytes, range_size)?;
        self.fetch_book_range_to_file(url, acquisition, inclusive_range_header(entry.header_start, range_size)?, validator, total_length, entry.header_start, range_size, path).await?;
        Ok(transferred_bytes)
    }

    async fn fetch_book_range(&self, url: &Url, acquisition: &OpdsDirectAcquisition, range: String, validator: Option<&str>, expected_total: Option<u64>, response_limit: u64) -> Result<OpdsBookRange, OpdsError> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::ACCEPT, reqwest::header::HeaderValue::from_static(EPUB_MEDIA_TYPE));
        headers.insert(reqwest::header::ACCEPT_ENCODING, reqwest::header::HeaderValue::from_static("identity"));
        headers.insert(reqwest::header::RANGE, reqwest::header::HeaderValue::from_str(&range).map_err(|_| OpdsError::InvalidCatalog("invalid EPUB byte range".to_owned()))?);
        if let Some(validator) = validator {
            headers.insert(reqwest::header::IF_RANGE, reqwest::header::HeaderValue::from_str(validator).map_err(|_| OpdsError::InvalidCatalog("invalid EPUB range validator".to_owned()))?);
        }
        litteraturbanken::decorate_download_headers(&mut headers, url);
        let response = self.send(url.clone(), headers).await?;
        if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(OpdsError::RangeUnsupported);
        }
        let final_url = validated_response_url(&response)?;
        let content_range = response.headers().get(reqwest::header::CONTENT_RANGE).and_then(|value| value.to_str().ok()).ok_or_else(|| OpdsError::InvalidCatalog("EPUB range response omitted Content-Range".to_owned()))?;
        let (start, end, total_length) = parse_content_range(content_range)?;
        if expected_total.is_some_and(|expected| expected != total_length) {
            return Err(OpdsError::InvalidCatalog("EPUB changed during ranged metadata extraction".to_owned()));
        }
        let validator = response.headers().get(reqwest::header::ETAG).or_else(|| response.headers().get(reqwest::header::LAST_MODIFIED)).and_then(|value| value.to_str().ok()).map(str::to_owned);
        let expected_length = end.checked_sub(start).and_then(|value| value.checked_add(1)).ok_or_else(|| OpdsError::InvalidCatalog("invalid EPUB response range".to_owned()))?;
        if expected_length > response_limit {
            return Err(OpdsError::ResponseTooLarge { limit: response_limit });
        }
        let bytes = read_bounded(response, response_limit).await?;
        if bytes.len() as u64 != expected_length {
            return Err(OpdsError::InvalidCatalog("EPUB range response length did not match Content-Range".to_owned()));
        }
        let _ = acquisition;
        Ok(OpdsBookRange { final_url, start, total_length, validator, bytes })
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn fetch_book_range_to_file(&self, url: &Url, acquisition: &OpdsDirectAcquisition, range: String, validator: Option<&str>, expected_total: u64, expected_start: u64, response_limit: u64, path: &Path) -> Result<(), OpdsError> {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::ACCEPT, reqwest::header::HeaderValue::from_static(EPUB_MEDIA_TYPE));
        headers.insert(reqwest::header::ACCEPT_ENCODING, reqwest::header::HeaderValue::from_static("identity"));
        headers.insert(reqwest::header::RANGE, reqwest::header::HeaderValue::from_str(&range).map_err(|_| OpdsError::InvalidCatalog("invalid EPUB byte range".to_owned()))?);
        if let Some(validator) = validator {
            headers.insert(reqwest::header::IF_RANGE, reqwest::header::HeaderValue::from_str(validator).map_err(|_| OpdsError::InvalidCatalog("invalid EPUB range validator".to_owned()))?);
        }
        litteraturbanken::decorate_download_headers(&mut headers, url);
        let response = self.send(url.clone(), headers).await?;
        if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(OpdsError::RangeUnsupported);
        }
        let _ = validated_response_url(&response)?;
        let content_range = response.headers().get(reqwest::header::CONTENT_RANGE).and_then(|value| value.to_str().ok()).ok_or_else(|| OpdsError::InvalidCatalog("EPUB range response omitted Content-Range".to_owned()))?;
        let (start, end, total_length) = parse_content_range(content_range)?;
        let expected_length = end.checked_sub(start).and_then(|value| value.checked_add(1)).ok_or_else(|| OpdsError::InvalidCatalog("invalid EPUB response range".to_owned()))?;
        if start != expected_start || total_length != expected_total || expected_length != response_limit {
            return Err(OpdsError::InvalidCatalog("EPUB entry range response did not match the requested archive bytes".to_owned()));
        }
        let mut file = tokio::fs::OpenOptions::new().write(true).open(path).await?;
        file.seek(std::io::SeekFrom::Start(start)).await?;
        let mut stream = response.bytes_stream();
        let mut written = 0_u64;
        while let Some(chunk) = tokio::time::timeout(NETWORK_IDLE_TIMEOUT, stream.next()).await.map_err(|_| OpdsError::TimedOut)? {
            let chunk = chunk?;
            written = written.checked_add(chunk.len() as u64).ok_or(OpdsError::ResponseTooLarge { limit: response_limit })?;
            if written > response_limit {
                return Err(OpdsError::ResponseTooLarge { limit: response_limit });
            }
            file.write_all(&chunk).await?;
        }
        if written != response_limit {
            return Err(OpdsError::InvalidCatalog("EPUB entry range response ended early".to_owned()));
        }
        file.flush().await?;
        let _ = acquisition;
        Ok(())
    }

    /// Downloads a catalog acquisition into memory for clients that own their
    /// storage, such as the browser guest runtime. The same media-type and size
    /// checks as the file-backed importer are applied, but no temporary file is
    /// created on the server.
    pub async fn download_book_bytes(&self, acquisition: &OpdsDirectAcquisition) -> Result<Vec<u8>, OpdsError> {
        if !is_importable_acquisition(acquisition) {
            return Err(OpdsError::NonFreeAcquisition);
        }
        let url = parse_http_url(&acquisition.url)?;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::ACCEPT, reqwest::header::HeaderValue::from_static(acquisition.book_format.media_type()));
        litteraturbanken::decorate_download_headers(&mut headers, &url);
        let response = self.send(url.clone(), headers).await?;
        let _ = validated_response_url(&response)?;
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(normalized_media_type);
        if media_type.as_deref().is_some_and(|value| !acquisition.book_format.accepts_media_type(value) && !matches!(value, "application/octet-stream" | "application/zip" | "binary/octet-stream")) {
            return Err(OpdsError::UnsupportedBookResponse(media_type));
        }
        let bytes = read_bounded(response, MAX_OPDS_DOWNLOAD_BYTES).await?;
        if bytes.is_empty() {
            return Err(OpdsError::InvalidCatalog("book acquisition returned an empty response".to_owned()));
        }
        Ok(bytes)
    }

    async fn fetch_catalog_url(&self, url: Url, byte_limit: u64) -> Result<OpdsCatalog, OpdsError> {
        if litteraturbanken::is_catalog_url(&url) {
            return litteraturbanken::fetch_catalog(self.outbound_policy, url, byte_limit).await;
        }
        let started = Instant::now();
        let (result, mut profile) = self.fetch_catalog_url_profiled(url, byte_limit).await;
        profile.total = started.elapsed();
        #[cfg(debug_assertions)]
        log_catalog_fetch_profile(&profile, result.as_ref().err());
        result
    }

    async fn fetch_catalog_url_conditional(&self, url: Url, byte_limit: u64, validators: &OpdsHttpValidators) -> Result<OpdsConditionalCatalog, OpdsError> {
        if litteraturbanken::is_catalog_url(&url) {
            let catalog = litteraturbanken::fetch_catalog(self.outbound_policy, url, byte_limit).await?;
            return Ok(OpdsConditionalCatalog::Modified { catalog, validators: OpdsHttpValidators::default() });
        }
        let mut url = url;
        for discovery_attempt in 0..=1 {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::ACCEPT,
                reqwest::header::HeaderValue::from_str(&format!("{OPDS_2_MEDIA_TYPE}, application/atom+xml;profile=opds-catalog, application/atom+xml;q=0.9, text/html;q=0.2")).expect("static OPDS Accept value is valid"),
            );
            if discovery_attempt == 0 {
                if let Some(etag) = validators.etag.as_deref() {
                    headers.insert(reqwest::header::IF_NONE_MATCH, reqwest::header::HeaderValue::from_str(etag).map_err(|_| OpdsError::InvalidCatalog("cached ETag is not a valid HTTP header value".to_owned()))?);
                }
                if let Some(last_modified) = validators.last_modified.as_deref() {
                    headers.insert(reqwest::header::IF_MODIFIED_SINCE, reqwest::header::HeaderValue::from_str(last_modified).map_err(|_| OpdsError::InvalidCatalog("cached Last-Modified value is not a valid HTTP header value".to_owned()))?);
                }
            }
            let response = self.get(url, headers).await?;
            if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                return Ok(OpdsConditionalCatalog::NotModified);
            }
            if !response.status().is_success() {
                return Err(http_status_error(&response));
            }
            let final_url = validated_response_url(&response)?;
            let response_validators = response_validators(&response);
            let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(normalized_media_type);
            let link_headers = response.headers().get_all(reqwest::header::LINK).iter().filter_map(|value| value.to_str().ok()).map(str::to_owned).collect::<Vec<_>>();
            let bytes = read_bounded(response, byte_limit).await?;
            if discovery_attempt == 0 && !looks_like_atom(&bytes) && (media_type.as_deref() == Some("text/html") || looks_like_html(&bytes)) {
                url = discover_opds_url(&final_url, &link_headers, &bytes).ok_or(OpdsError::UnsupportedCatalogFormat)?;
                continue;
            }
            return Ok(OpdsConditionalCatalog::Modified { catalog: parse_catalog(final_url, media_type.as_deref(), &bytes)?, validators: response_validators });
        }
        Err(OpdsError::UnsupportedCatalogFormat)
    }

    async fn fetch_catalog_url_profiled(&self, url: Url, byte_limit: u64) -> (Result<OpdsCatalog, OpdsError>, OpdsFetchProfile) {
        let mut profile = OpdsFetchProfile::new(url.to_string());
        let mut url = url;
        for discovery_attempt in 0..=1 {
            profile.http_fetches += 1;
            let headers_started = Instant::now();
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::ACCEPT,
                reqwest::header::HeaderValue::from_str(&format!("{OPDS_2_MEDIA_TYPE}, application/atom+xml;profile=opds-catalog, application/atom+xml;q=0.9, text/html;q=0.2")).expect("static OPDS Accept value is valid"),
            );
            let response = self.send(url, headers).await;
            profile.response_headers += headers_started.elapsed();
            let response = match response {
                Ok(response) => response,
                Err(error) => return (Err(error), profile),
            };
            let final_url = match validated_response_url(&response) {
                Ok(url) => url,
                Err(error) => return (Err(error), profile),
            };
            profile.final_url = Some(final_url.to_string());
            let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(normalized_media_type);
            let link_headers = response.headers().get_all(reqwest::header::LINK).iter().filter_map(|value| value.to_str().ok()).map(str::to_owned).collect::<Vec<_>>();
            let download_started = Instant::now();
            let bytes = read_bounded(response, byte_limit).await;
            profile.body_download += download_started.elapsed();
            let bytes = match bytes {
                Ok(bytes) => bytes,
                Err(error) => return (Err(error), profile),
            };
            profile.response_bytes = profile.response_bytes.saturating_add(bytes.len());
            let discovery_started = Instant::now();
            if discovery_attempt == 0 && !looks_like_atom(&bytes) && (media_type.as_deref() == Some("text/html") || looks_like_html(&bytes)) {
                let discovered = discover_opds_url(&final_url, &link_headers, &bytes);
                profile.discovery += discovery_started.elapsed();
                let Some(discovered) = discovered else { return (Err(OpdsError::UnsupportedCatalogFormat), profile) };
                url = discovered;
                continue;
            }
            profile.discovery += discovery_started.elapsed();
            let parsing_started = Instant::now();
            let result = parse_catalog(final_url, media_type.as_deref(), &bytes);
            profile.parsing += parsing_started.elapsed();
            return (result, profile);
        }
        (Err(OpdsError::UnsupportedCatalogFormat), profile)
    }

    async fn get(&self, url: Url, headers: reqwest::header::HeaderMap) -> Result<Response, OpdsError> {
        outbound::send_get(url, headers, self.outbound_policy, &format!("Bokheim/{} OPDS", env!("CARGO_PKG_VERSION")), Duration::from_secs(10), NETWORK_IDLE_TIMEOUT).await.map_err(|error| OpdsError::InvalidUrl(error.to_string()))
    }

    async fn send(&self, url: Url, headers: reqwest::header::HeaderMap) -> Result<Response, OpdsError> {
        let response = self.get(url, headers).await?;
        if !response.status().is_success() {
            return Err(http_status_error(&response));
        }
        Ok(response)
    }
}

fn response_validators(response: &Response) -> OpdsHttpValidators {
    OpdsHttpValidators {
        etag: response.headers().get(reqwest::header::ETAG).and_then(|value| value.to_str().ok()).map(str::to_owned),
        last_modified: response.headers().get(reqwest::header::LAST_MODIFIED).and_then(|value| value.to_str().ok()).map(str::to_owned),
    }
}

fn http_status_error(response: &Response) -> OpdsError {
    OpdsError::HttpStatus { status: response.status(), retry_after: retry_after(response.headers(), SystemTime::now()) }
}

fn retry_after(headers: &reqwest::header::HeaderMap, now: SystemTime) -> Option<Duration> {
    let value = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    Some(httpdate::parse_http_date(value).ok()?.duration_since(now).unwrap_or(Duration::ZERO))
}

#[cfg(debug_assertions)]
fn log_catalog_fetch_profile(profile: &OpdsFetchProfile, error: Option<&OpdsError>) {
    let outcome = error.map(opds_error_kind).unwrap_or("ok");
    log::info!(target: "performance", "opds.fetch url={} outcome={} total_ms={:.2} headers_ms={:.2} download_ms={:.2} discovery_ms={:.2} parse_ms={:.2} bytes={} http_fetches={}", profile_log_url(&profile.requested_url), outcome, duration_ms(profile.total), duration_ms(profile.response_headers), duration_ms(profile.body_download), duration_ms(profile.discovery), duration_ms(profile.parsing), profile.response_bytes, profile.http_fetches);
}

#[cfg(debug_assertions)]
fn opds_error_kind(error: &OpdsError) -> &'static str {
    match error {
        OpdsError::InvalidEndpoint(_) => "invalid_endpoint",
        OpdsError::InvalidUrl(_) => "invalid_url",
        OpdsError::Network(_) => "network_error",
        OpdsError::TimedOut => "timeout",
        OpdsError::HttpStatus { .. } => "http_error",
        OpdsError::ResponseTooLarge { .. } => "response_too_large",
        OpdsError::RangeUnsupported => "range_unsupported",
        OpdsError::UnsupportedCatalogFormat => "unsupported_catalog_format",
        OpdsError::InvalidCatalog(_) => "invalid_catalog",
        OpdsError::NonFreeAcquisition => "non_free_acquisition",
        OpdsError::UnsupportedBookResponse(_) => "unsupported_book_response",
        OpdsError::Io(_) => "io_error",
    }
}

#[cfg(any(debug_assertions, test))]
fn profile_log_url(value: &str) -> String {
    let Ok(mut url) = Url::parse(value) else { return "invalid-url".to_owned() };
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

#[cfg(debug_assertions)]
fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

struct OpdsBookRange {
    final_url: Url,
    start: u64,
    total_length: u64,
    validator: Option<String>,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
struct RemoteZipEntry {
    header_start: u64,
    compressed_size: u64,
    uncompressed_size: u64,
    encrypted: bool,
}

struct EpubRelatedEntries {
    navigation: Vec<String>,
    cover: Option<String>,
}

fn little_u16(bytes: &[u8], offset: usize) -> Result<u16, OpdsError> {
    let value = bytes.get(offset..offset + 2).ok_or_else(|| OpdsError::InvalidCatalog("truncated EPUB ZIP metadata".to_owned()))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn little_u32(bytes: &[u8], offset: usize) -> Result<u32, OpdsError> {
    let value = bytes.get(offset..offset + 4).ok_or_else(|| OpdsError::InvalidCatalog("truncated EPUB ZIP metadata".to_owned()))?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn parse_content_range(value: &str) -> Result<(u64, u64, u64), OpdsError> {
    let value = value.strip_prefix("bytes ").ok_or_else(|| OpdsError::InvalidCatalog("invalid EPUB Content-Range unit".to_owned()))?;
    let (range, total) = value.split_once('/').ok_or_else(|| OpdsError::InvalidCatalog("invalid EPUB Content-Range".to_owned()))?;
    let (start, end) = range.split_once('-').ok_or_else(|| OpdsError::InvalidCatalog("invalid EPUB Content-Range bounds".to_owned()))?;
    let start = start.parse::<u64>().map_err(|_| OpdsError::InvalidCatalog("invalid EPUB Content-Range start".to_owned()))?;
    let end = end.parse::<u64>().map_err(|_| OpdsError::InvalidCatalog("invalid EPUB Content-Range end".to_owned()))?;
    let total = total.parse::<u64>().map_err(|_| OpdsError::InvalidCatalog("invalid EPUB Content-Range total".to_owned()))?;
    if start > end || end >= total {
        return Err(OpdsError::InvalidCatalog("inconsistent EPUB Content-Range".to_owned()));
    }
    Ok((start, end, total))
}

fn inclusive_range_header(start: u64, size: u64) -> Result<String, OpdsError> {
    let end = start.checked_add(size).and_then(|value| value.checked_sub(1)).ok_or_else(|| OpdsError::InvalidCatalog("EPUB byte range overflowed".to_owned()))?;
    Ok(format!("bytes={start}-{end}"))
}

fn checked_range_budget(current: u64, additional: u64) -> Result<u64, OpdsError> {
    let total = current.checked_add(additional).ok_or(OpdsError::ResponseTooLarge { limit: MAX_EPUB_RANGE_BYTES })?;
    if total > MAX_EPUB_RANGE_BYTES {
        return Err(OpdsError::ResponseTooLarge { limit: MAX_EPUB_RANGE_BYTES });
    }
    Ok(total)
}

fn epub_central_directory_bounds(tail: &[u8], tail_start: u64, total_length: u64) -> Result<(u64, u64), OpdsError> {
    let position = tail.windows(4).rposition(|window| window == b"PK\x05\x06").ok_or_else(|| OpdsError::InvalidCatalog("EPUB ZIP end record was not found".to_owned()))?;
    if little_u16(tail, position + 4)? != 0 || little_u16(tail, position + 6)? != 0 {
        return Err(OpdsError::InvalidCatalog("multi-disk EPUB ZIP archives are unsupported".to_owned()));
    }
    let entries = little_u16(tail, position + 10)?;
    let central_size = little_u32(tail, position + 12)?;
    let central_offset = little_u32(tail, position + 16)?;
    if entries == u16::MAX || central_size == u32::MAX || central_offset == u32::MAX {
        return Err(OpdsError::InvalidCatalog("ZIP64 EPUB metadata requires a full-download fallback".to_owned()));
    }
    let comment_length = little_u16(tail, position + 20)? as usize;
    if position + 22 + comment_length > tail.len() || tail_start + position as u64 + 22 + comment_length as u64 > total_length {
        return Err(OpdsError::InvalidCatalog("truncated EPUB ZIP end record".to_owned()));
    }
    let central_offset = central_offset as u64;
    let central_size = central_size as u64;
    if central_offset.checked_add(central_size).is_none_or(|end| end > total_length) {
        return Err(OpdsError::InvalidCatalog("EPUB ZIP directory exceeds the archive".to_owned()));
    }
    Ok((central_offset, central_size))
}

fn parse_epub_central_directory(bytes: &[u8], central_offset: u64) -> Result<HashMap<String, RemoteZipEntry>, OpdsError> {
    let mut entries = HashMap::new();
    let mut cursor = 0_usize;
    while cursor < bytes.len() {
        if bytes.len() - cursor < 46 || bytes.get(cursor..cursor + 4) != Some(b"PK\x01\x02".as_slice()) {
            return Err(OpdsError::InvalidCatalog(format!("invalid EPUB ZIP directory at byte {}", central_offset + cursor as u64)));
        }
        let flags = little_u16(bytes, cursor + 8)?;
        let compressed_size = little_u32(bytes, cursor + 20)?;
        let uncompressed_size = little_u32(bytes, cursor + 24)?;
        let name_length = little_u16(bytes, cursor + 28)? as usize;
        let extra_length = little_u16(bytes, cursor + 30)? as usize;
        let comment_length = little_u16(bytes, cursor + 32)? as usize;
        let header_start = little_u32(bytes, cursor + 42)?;
        if compressed_size == u32::MAX || uncompressed_size == u32::MAX || header_start == u32::MAX {
            return Err(OpdsError::InvalidCatalog("ZIP64 EPUB entries require a full-download fallback".to_owned()));
        }
        let record_length = 46_usize
            .checked_add(name_length)
            .and_then(|value| value.checked_add(extra_length))
            .and_then(|value| value.checked_add(comment_length))
            .ok_or_else(|| OpdsError::InvalidCatalog("EPUB ZIP directory length overflowed".to_owned()))?;
        let name_bytes = bytes.get(cursor + 46..cursor + 46 + name_length).ok_or_else(|| OpdsError::InvalidCatalog("truncated EPUB ZIP file name".to_owned()))?;
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        entries.insert(name, RemoteZipEntry { header_start: header_start as u64, compressed_size: compressed_size as u64, uncompressed_size: uncompressed_size as u64, encrypted: flags & 1 != 0 });
        cursor = cursor.checked_add(record_length).ok_or_else(|| OpdsError::InvalidCatalog("EPUB ZIP directory cursor overflowed".to_owned()))?;
    }
    Ok(entries)
}

#[cfg(not(target_arch = "wasm32"))]
async fn write_sparse_range(path: &Path, start: u64, bytes: &[u8]) -> Result<(), OpdsError> {
    let mut file = tokio::fs::OpenOptions::new().write(true).open(path).await?;
    file.seek(std::io::SeekFrom::Start(start)).await?;
    file.write_all(bytes).await?;
    file.flush().await?;
    Ok(())
}

fn read_epub_entry(path: &Path, name: &str, limit: u64) -> Result<Vec<u8>, OpdsError> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| OpdsError::InvalidCatalog(error.to_string()))?;
    let entry = archive.by_name(name).map_err(|error| OpdsError::InvalidCatalog(error.to_string()))?;
    if entry.size() > limit {
        return Err(OpdsError::ResponseTooLarge { limit });
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(OpdsError::ResponseTooLarge { limit });
    }
    Ok(bytes)
}

fn epub_xml_attribute(decoder: quick_xml::encoding::Decoder, event: &quick_xml::events::BytesStart<'_>, suffix: &[u8]) -> Option<String> {
    event
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.as_ref().ends_with(suffix))
        .map(|attribute| attribute.decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder).map_or_else(|_| String::from_utf8_lossy(&attribute.value).into_owned(), std::borrow::Cow::into_owned))
}

fn epub_package_path(container: &[u8]) -> Result<String, OpdsError> {
    let mut reader = Reader::from_reader(container);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event) | Event::Empty(event)) if event.name().as_ref().ends_with(b"rootfile") => {
                let path = epub_xml_attribute(reader.decoder(), &event, b"full-path").and_then(|value| normalize_epub_entry_path("", &value));
                return path.ok_or_else(|| OpdsError::InvalidCatalog("EPUB container has no valid package path".to_owned()));
            }
            Ok(Event::Eof) => return Err(OpdsError::InvalidCatalog("EPUB container has no package document".to_owned())),
            Err(error) => return Err(OpdsError::InvalidCatalog(error.to_string())),
            _ => {}
        }
        buffer.clear();
    }
}

fn normalize_epub_entry_path(base: &str, href: &str) -> Option<String> {
    let href = href.split(['#', '?']).next().unwrap_or_default();
    let href = urlencoding::decode(href).ok()?;
    let joined = if href.starts_with('/') {
        href.trim_start_matches('/').to_owned()
    } else if base.is_empty() {
        href.into_owned()
    } else {
        format!("{base}/{href}")
    };
    let mut components = Vec::new();
    for component in joined.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop()?;
            }
            value => components.push(value),
        }
    }
    (!components.is_empty()).then(|| components.join("/"))
}

fn epub_package_related_entries(package_name: &str, package: &[u8]) -> Result<EpubRelatedEntries, OpdsError> {
    #[derive(Clone)]
    struct Item {
        id: String,
        path: String,
        media_type: String,
        properties: String,
    }
    let base = package_name.rsplit_once('/').map(|(base, _)| base).unwrap_or("");
    let mut reader = Reader::from_reader(package);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut items = Vec::<Item>::new();
    let mut cover_id = None;
    let mut spine_toc_id = None;
    let mut guide_cover = None;
    let mut in_guide = false;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event) | Event::Empty(event)) => {
                let name = event.name();
                if name.as_ref().ends_with(b"guide") {
                    in_guide = true;
                } else if name.as_ref().ends_with(b"item") {
                    if let (Some(id), Some(href)) = (epub_xml_attribute(reader.decoder(), &event, b"id"), epub_xml_attribute(reader.decoder(), &event, b"href")) {
                        if let Some(path) = normalize_epub_entry_path(base, &href) {
                            items.push(Item {
                                id,
                                path,
                                media_type: epub_xml_attribute(reader.decoder(), &event, b"media-type").unwrap_or_default(),
                                properties: epub_xml_attribute(reader.decoder(), &event, b"properties").unwrap_or_default(),
                            });
                        }
                    }
                } else if name.as_ref().ends_with(b"meta") && epub_xml_attribute(reader.decoder(), &event, b"name").as_deref() == Some("cover") {
                    cover_id = epub_xml_attribute(reader.decoder(), &event, b"content");
                } else if name.as_ref().ends_with(b"spine") {
                    spine_toc_id = epub_xml_attribute(reader.decoder(), &event, b"toc");
                } else if in_guide && name.as_ref().ends_with(b"reference") && epub_xml_attribute(reader.decoder(), &event, b"type").is_some_and(|value| value.split_whitespace().any(|value| value.eq_ignore_ascii_case("cover"))) {
                    guide_cover = epub_xml_attribute(reader.decoder(), &event, b"href").and_then(|href| normalize_epub_entry_path(base, &href));
                }
            }
            Ok(Event::End(event)) if event.name().as_ref().ends_with(b"guide") => in_guide = false,
            Ok(Event::Eof) => break,
            Err(error) => return Err(OpdsError::InvalidCatalog(error.to_string())),
            _ => {}
        }
        buffer.clear();
    }
    let mut navigation = Vec::new();
    for item in &items {
        if item.properties.split_whitespace().any(|property| property == "nav") || item.media_type == "application/x-dtbncx+xml" || spine_toc_id.as_deref() == Some(item.id.as_str()) {
            if !navigation.contains(&item.path) {
                navigation.push(item.path.clone());
            }
        }
    }
    let is_image = |item: &&Item| item.media_type.starts_with("image/");
    let cover = items
        .iter()
        .find(|item| item.properties.split_whitespace().any(|property| property == "cover-image") && item.media_type.starts_with("image/"))
        .or_else(|| cover_id.as_deref().and_then(|id| items.iter().find(|item| item.id == id)).filter(is_image))
        .map(|item| item.path.clone())
        .or_else(|| guide_cover.filter(|path| items.iter().any(|item| &item.path == path && item.media_type.starts_with("image/"))))
        .or_else(|| items.iter().find(|item| item.media_type.starts_with("image/") && (item.id.to_ascii_lowercase().contains("cover") || item.path.to_ascii_lowercase().contains("cover"))).map(|item| item.path.clone()));
    Ok(EpubRelatedEntries { navigation, cover })
}

pub struct DownloadedOpdsBook {
    path: PathBuf,
    file_name: String,
    transferred_bytes: u64,
    metadata_only: bool,
}

impl DownloadedOpdsBook {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Bytes transferred over HTTP. A sparse metadata-only EPUB can have a
    /// much larger logical file length while occupying only these fetched
    /// archive ranges on disk.
    pub fn transferred_bytes(&self) -> u64 {
        self.transferred_bytes
    }

    pub fn is_metadata_only(&self) -> bool {
        self.metadata_only
    }
}

impl Drop for DownloadedOpdsBook {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

async fn read_bounded(response: Response, limit: u64) -> Result<Vec<u8>, OpdsError> {
    if response.content_length().is_some_and(|length| length > limit) {
        return Err(OpdsError::ResponseTooLarge { limit });
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = tokio::time::timeout(NETWORK_IDLE_TIMEOUT, stream.next()).await.map_err(|_| OpdsError::TimedOut)? {
        let chunk = chunk?;
        if (bytes.len() as u64).saturating_add(chunk.len() as u64) > limit {
            return Err(OpdsError::ResponseTooLarge { limit });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn parse_catalog(source_url: Url, media_type: Option<&str>, bytes: &[u8]) -> Result<OpdsCatalog, OpdsError> {
    let first = bytes.iter().copied().find(|byte| !byte.is_ascii_whitespace());
    if media_type == Some(OPDS_2_MEDIA_TYPE) || first == Some(b'{') {
        return parse_opds_2(source_url, bytes);
    }
    if media_type.is_some_and(|value| value == "application/atom+xml" || value == "application/xml" || value == "text/xml") || first == Some(b'<') {
        return parse_opds_1(source_url, bytes);
    }
    Err(OpdsError::UnsupportedCatalogFormat)
}

fn parse_opds_1(source_url: Url, bytes: &[u8]) -> Result<OpdsCatalog, OpdsError> {
    let bytes = trim_after_atom_document(bytes);
    let feed = parse_atom_document(bytes)?;
    let acquisition_details = parse_opds_1_acquisition_details(bytes)?;
    let base_url = match feed.base.as_deref() {
        Some(base) => resolve_url(&source_url, base)?,
        None => source_url.clone(),
    };
    let mut navigation = Vec::new();
    let mut publications = Vec::new();
    for (entry_index, entry) in feed.entries.iter().enumerate() {
        let acquisition_links = entry.links.iter().filter(|link| acquisition_kind(&link.rel).is_some()).collect::<Vec<_>>();
        if acquisition_links.is_empty() {
            if let Some(link) = entry.links.iter().find(|link| link.mime_type.as_deref().is_some_and(is_catalog_media_type) || is_navigation_relation(&link.rel)) {
                if let Ok(url) = resolve_url(&base_url, &link.href) {
                    let target = entry.links.iter().find(|candidate| candidate.href == link.href).and_then(|candidate| candidate.mime_type.as_deref()).map(link_target_from_media_type).unwrap_or(OpdsLinkTarget::Catalog);
                    navigation.push(OpdsCatalogLink { title: nonempty(entry.title.as_str()).unwrap_or("Untitled").to_owned(), url: url.to_string(), target, relations: vec![link.rel.clone()], number_of_items: None, active: false });
                }
            }
            continue;
        }
        let title = nonempty(entry.title.as_str()).unwrap_or("Untitled").to_owned();
        let acquisitions =
            acquisition_links.into_iter().enumerate().filter_map(|(link_index, link)| acquisition_from_atom(&base_url, link, &title, acquisition_details.get(entry_index).and_then(|links| links.get(link_index)))).collect::<Vec<_>>();
        let preferred_download = preferred_download(&acquisitions);
        let authors = entry
            .authors
            .iter()
            .filter_map(|author| {
                let name = nonempty(&author.name)?.to_owned();
                let raw_uri = author.uri.as_deref().and_then(nonempty);
                let resolved_uri = raw_uri.and_then(|uri| resolve_url(&base_url, uri).ok()).map(|url| url.to_string());
                let identifier = resolved_uri.clone().or_else(|| raw_uri.map(str::to_owned));
                Some(OpdsAuthorCredit {
                    name,
                    name_localizations: Vec::new(),
                    sort_name: None,
                    sort_name_localizations: Vec::new(),
                    identifiers: identifier.into_iter().collect(),
                    provider_link: resolved_uri.map(|url| OpdsAuthorLink { url, target: OpdsLinkTarget::Web }),
                })
            })
            .collect::<Vec<_>>();
        let self_url = relation_url_atom(&base_url, &entry.links, "self");
        let id = stable_publication_id(Some(&entry.id), self_url.as_deref(), &acquisitions, &base_url, &title, &authors);
        let description = atom_description(entry).and_then(|description| normalize_project_gutenberg_description(&id, description));
        let languages = atom_publication_languages(entry);
        let work_id = work_id_from_publication_id(&id);
        let mut metadata = atom_publication_metadata(&base_url, entry);
        append_identity_evidence(&mut metadata, &id, work_id.as_deref(), "opds1:resolved-identity");
        let image_url = preferred_image_url(&metadata.images);
        publications.push(OpdsPublication { work_id, id, title, authors, languages, description, image_url, metadata, preferred_download, acquisitions });
    }
    let facets = parse_opds_1_facets(&base_url, bytes)?;
    Ok(OpdsCatalog {
        version: OpdsVersion::V1_2,
        freshness: OpdsCatalogFreshness::Current,
        title: nonempty(feed.title.as_str()).unwrap_or("Untitled catalog").to_owned(),
        source_url: source_url.to_string(),
        self_url: relation_url_atom(&base_url, &feed.links, "self"),
        start_url: relation_url_atom(&base_url, &feed.links, "start"),
        navigation,
        publications,
        groups: Vec::new(),
        facets,
        pagination: OpdsPagination {
            first: relation_url_atom(&base_url, &feed.links, "first"),
            previous: relation_url_atom(&base_url, &feed.links, "previous").or_else(|| relation_url_atom(&base_url, &feed.links, "prev")),
            next: relation_url_atom(&base_url, &feed.links, "next"),
            last: relation_url_atom(&base_url, &feed.links, "last"),
            ..OpdsPagination::default()
        },
    })
}

fn parse_opds_1_facets(base: &Url, bytes: &[u8]) -> Result<Vec<OpdsFacet>, OpdsError> {
    let mut reader = Reader::from_reader(bytes);
    let mut entry_depth = 0_u32;
    let mut facets = Vec::<OpdsFacet>::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) if element.local_name().as_ref() == b"entry" => entry_depth = entry_depth.saturating_add(1),
            Ok(Event::End(element)) if element.local_name().as_ref() == b"entry" => entry_depth = entry_depth.saturating_sub(1),
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) if element.local_name().as_ref() == b"link" && entry_depth == 0 => {
                if xml_attribute(&reader, &element, b"rel")?.as_deref() != Some("http://opds-spec.org/facet") {
                    continue;
                }
                let Some(href) = xml_attribute(&reader, &element, b"href")? else { continue };
                let Ok(url) = resolve_url(base, &href) else { continue };
                let title = xml_attribute(&reader, &element, b"title")?.and_then(|value| nonempty(&value).map(str::to_owned)).unwrap_or_else(|| "Untitled".to_owned());
                let group = xml_attribute(&reader, &element, b"facetGroup")?.and_then(|value| nonempty(&value).map(str::to_owned)).unwrap_or_else(|| "Facets".to_owned());
                let active = xml_attribute(&reader, &element, b"activeFacet")?.is_some_and(|value| value.eq_ignore_ascii_case("true"));
                let number_of_items = xml_attribute(&reader, &element, b"count")?.and_then(|value| value.parse::<u32>().ok());
                let link = OpdsCatalogLink { title, url: url.to_string(), target: OpdsLinkTarget::Catalog, relations: vec!["http://opds-spec.org/facet".to_owned()], number_of_items, active };
                if let Some(facet) = facets.iter_mut().find(|facet| facet.title == group) {
                    facet.links.push(link);
                } else {
                    facets.push(OpdsFacet { title: group, links: vec![link] });
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(OpdsError::InvalidCatalog(error.to_string())),
        }
    }
    Ok(facets)
}

fn trim_after_atom_document(bytes: &[u8]) -> &[u8] {
    for closing in [b"</feed>".as_slice(), b"</entry>".as_slice()] {
        if let Some(index) = bytes.windows(closing.len()).position(|window| window.eq_ignore_ascii_case(closing)) {
            let end = index + closing.len();
            return if bytes[end..].iter().any(|byte| !byte.is_ascii_whitespace()) { &bytes[..end] } else { bytes };
        }
    }
    bytes
}

fn parse_atom_document(bytes: &[u8]) -> Result<Feed, OpdsError> {
    fn parse(bytes: &[u8]) -> Result<Feed, atom_syndication::Error> {
        let mut reader = Reader::from_reader(bytes);
        let root = loop {
            match reader.read_event() {
                Ok(Event::Start(element)) => break element.local_name().as_ref().to_vec(),
                Ok(Event::Eof) | Err(_) => break Vec::new(),
                Ok(_) => {}
            }
        };
        if root == b"entry" {
            let entry = AtomEntry::read_from(Cursor::new(bytes))?;
            Ok(Feed { title: entry.title.clone(), id: entry.id.clone(), updated: entry.updated, entries: vec![entry], ..Feed::default() })
        } else {
            Feed::read_from(Cursor::new(bytes))
        }
    }
    match parse(bytes) {
        Ok(feed) => Ok(feed),
        Err(original_error) => {
            let repaired = String::from_utf8_lossy(bytes).replacen("xmlns=\"https://www.w3.org/2005/Atom\"", "xmlns=\"http://www.w3.org/2005/Atom\"", 1);
            let repaired = escape_non_xml_named_entities(&repaired);
            let repaired = repair_year_only_atom_timestamps(&repaired);
            if repaired.as_bytes() == bytes {
                return Err(OpdsError::InvalidCatalog(original_error.to_string()));
            }
            parse(repaired.as_bytes()).map_err(|_| OpdsError::InvalidCatalog(original_error.to_string()))
        }
    }
}

fn repair_year_only_atom_timestamps(value: &str) -> String {
    let mut repaired = value.to_owned();
    for tag in ["published", "updated"] {
        let opening = format!("<{tag}>");
        let closing = format!("</{tag}>");
        let mut cursor = 0;
        while let Some(relative_start) = repaired[cursor..].find(&opening) {
            let value_start = cursor + relative_start + opening.len();
            let Some(relative_end) = repaired[value_start..].find(&closing) else { break };
            let value_end = value_start + relative_end;
            let timestamp = repaired[value_start..value_end].trim();
            if timestamp.len() == 4 && timestamp.bytes().all(|byte| byte.is_ascii_digit()) && timestamp != "0000" {
                let replacement = format!("{timestamp}-01-01T00:00:00Z");
                repaired.replace_range(value_start..value_end, &replacement);
                cursor = value_start + replacement.len() + closing.len();
            } else {
                cursor = value_end + closing.len();
            }
        }
    }
    repaired
}

fn escape_non_xml_named_entities(value: &str) -> String {
    const XML_ENTITIES: &[&str] = &["amp", "apos", "gt", "lt", "quot"];
    let mut repaired = String::with_capacity(value.len());
    let mut cursor = 0;
    while let Some(relative_start) = value[cursor..].find('&') {
        let start = cursor + relative_start;
        repaired.push_str(&value[cursor..start]);
        let Some(relative_end) = value[start + 1..].find(';') else {
            repaired.push_str(&value[start..]);
            return repaired;
        };
        let end = start + 1 + relative_end;
        let entity = &value[start + 1..end];
        if !entity.is_empty() && entity.len() <= 64 && entity.bytes().all(|byte| byte.is_ascii_alphanumeric()) && !XML_ENTITIES.contains(&entity) {
            repaired.push_str("&amp;");
            repaired.push_str(entity);
            repaired.push(';');
        } else {
            repaired.push_str(&value[start..=end]);
        }
        cursor = end + 1;
    }
    repaired.push_str(&value[cursor..]);
    repaired
}

fn parse_opds_2(source_url: Url, bytes: &[u8]) -> Result<OpdsCatalog, OpdsError> {
    let root: Value = serde_json::from_slice(bytes).map_err(|error| OpdsError::InvalidCatalog(error.to_string()))?;
    let object = root.as_object().ok_or_else(|| OpdsError::InvalidCatalog("feed root must be a JSON object".to_owned()))?;
    let metadata = object.get("metadata").and_then(Value::as_object).ok_or_else(|| OpdsError::InvalidCatalog("feed is missing metadata".to_owned()))?;
    let title = metadata.get("title").and_then(localized_text).and_then(|value| nonempty(&value).map(str::to_owned)).ok_or_else(|| OpdsError::InvalidCatalog("feed metadata is missing a title".to_owned()))?;
    let links = object.get("links").and_then(Value::as_array).cloned().unwrap_or_default();
    let navigation = parse_json_navigation(&source_url, object.get("navigation"));
    let publications = parse_json_publications(&source_url, object.get("publications"));
    let groups: Vec<OpdsGroup> = object.get("groups").and_then(Value::as_array).map(|groups| groups.iter().filter_map(|group| parse_json_group(&source_url, group)).collect()).unwrap_or_default();
    let facets: Vec<OpdsFacet> = object.get("facets").and_then(Value::as_array).map(|facets| facets.iter().filter_map(|facet| parse_json_facet(&source_url, facet)).collect()).unwrap_or_default();
    if navigation.is_empty() && publications.is_empty() && groups.is_empty() {
        return Err(OpdsError::InvalidCatalog("feed contains no navigation, publications, or groups".to_owned()));
    }
    Ok(OpdsCatalog {
        version: OpdsVersion::V2_0,
        freshness: OpdsCatalogFreshness::Current,
        title,
        source_url: source_url.to_string(),
        self_url: relation_url_json(&source_url, &links, "self"),
        start_url: relation_url_json(&source_url, &links, "start"),
        navigation,
        publications,
        groups,
        facets,
        pagination: OpdsPagination {
            first: relation_url_json(&source_url, &links, "first"),
            previous: relation_url_json(&source_url, &links, "previous").or_else(|| relation_url_json(&source_url, &links, "prev")),
            next: relation_url_json(&source_url, &links, "next"),
            last: relation_url_json(&source_url, &links, "last"),
            number_of_items: metadata.get("numberOfItems").and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok()),
            items_per_page: metadata.get("itemsPerPage").and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok()),
            current_page: metadata.get("currentPage").and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok()),
        },
    })
}

fn parse_json_navigation(base: &Url, value: Option<&Value>) -> Vec<OpdsCatalogLink> {
    value.and_then(Value::as_array).map(|links| links.iter().filter_map(|link| catalog_link_from_json(base, link)).collect()).unwrap_or_default()
}

fn parse_json_publications(base: &Url, value: Option<&Value>) -> Vec<OpdsPublication> {
    value.and_then(Value::as_array).map(|publications| publications.iter().filter_map(|publication| parse_json_publication(base, publication)).collect()).unwrap_or_default()
}

fn parse_json_group(base: &Url, value: &Value) -> Option<OpdsGroup> {
    let object = value.as_object()?;
    let title = object.get("metadata").and_then(Value::as_object).and_then(|metadata| metadata.get("title")).and_then(localized_text).unwrap_or_else(|| "Untitled group".to_owned());
    let links = object.get("links").and_then(Value::as_array).cloned().unwrap_or_default();
    let self_url = relation_url_json(base, &links, "self");
    let number_of_items = object.get("metadata").and_then(Value::as_object).and_then(|metadata| metadata.get("numberOfItems")).and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok());
    Some(OpdsGroup { title, self_url, number_of_items, navigation: parse_json_navigation(base, object.get("navigation")), publications: parse_json_publications(base, object.get("publications")) })
}

fn parse_json_facet(base: &Url, value: &Value) -> Option<OpdsFacet> {
    let object = value.as_object()?;
    let title = object.get("metadata").and_then(Value::as_object).and_then(|metadata| metadata.get("title")).and_then(localized_text).unwrap_or_else(|| "Facets".to_owned());
    let links = object.get("links").and_then(Value::as_array).map(|links| links.iter().filter_map(|link| catalog_link_from_json(base, link)).collect()).unwrap_or_default();
    Some(OpdsFacet { title, links })
}

fn parse_json_publication(base: &Url, value: &Value) -> Option<OpdsPublication> {
    let object = value.as_object()?;
    let metadata = object.get("metadata")?.as_object()?;
    let title = metadata.get("title").and_then(localized_text).and_then(|title| nonempty(&title).map(str::to_owned)).unwrap_or_else(|| "Untitled".to_owned());
    let links = object.get("links").and_then(Value::as_array).cloned().unwrap_or_default();
    let authors = metadata.get("author").map(|authors| contributor_credits(base, authors)).unwrap_or_default();
    let languages = metadata.get("language").map(publication_languages).unwrap_or_default();
    let description = metadata.get("description").and_then(localized_text).as_deref().and_then(inferred_description);
    let mut acquisitions = links.iter().filter_map(|link| acquisition_from_json(base, link, &title)).collect::<Vec<_>>();
    if acquisitions.is_empty() {
        if let Some(url) = links.iter().find(|link| json_link_has_relation(link, "alternate") && link.get("type").and_then(Value::as_str).is_some_and(|media_type| media_type == "text/html")).and_then(|link| json_link_url(base, link)) {
            acquisitions.push(acquisition(url, OpdsAcquisitionKind::Generic, Some("text/html"), &title, None, Vec::new()));
        }
    }
    let self_url = relation_url_json(base, &links, "self");
    let explicit_identifier = metadata.get("identifier").map(identifier_values).and_then(|identifiers| identifiers.into_iter().next());
    let id = stable_publication_id(explicit_identifier.as_deref(), self_url.as_deref(), &acquisitions, base, &title, &authors);
    let preferred_download = preferred_download(&acquisitions);
    let work_id = work_id_from_publication_id(&id);
    let mut metadata = json_publication_metadata(base, metadata);
    append_identity_evidence(&mut metadata, &id, work_id.as_deref(), "opds2:resolved-identity");
    metadata.images = json_publication_images(base, object);
    let image_url = preferred_image_url(&metadata.images);
    Some(OpdsPublication { work_id, id, title, authors, languages, description, image_url, metadata, preferred_download, acquisitions })
}

fn atom_publication_metadata(base: &Url, entry: &AtomEntry) -> OpdsPublicationMetadata {
    let publishers = atom_extensions(entry, "publisher")
        .filter_map(|extension| {
            nonempty(extension.value.as_deref()?).map(|name| OpdsAgentCredit { name: name.to_owned(), name_localizations: Vec::new(), sort_name: None, sort_name_localizations: Vec::new(), identifiers: Vec::new(), provider_link: None })
        })
        .collect::<Vec<_>>();
    let mut identifiers = vec![opds_identifier(&entry.id, None, "atom:id", OpdsIdentifierScope::CatalogEntry)];
    identifiers.extend(atom_extensions(entry, "identifier").filter_map(|extension| {
        let value = nonempty(extension.value.as_deref()?)?;
        let scheme = atom_extension_attribute(extension, &["scheme", "type"]).map(str::to_owned);
        Some(opds_identifier(value, scheme, &extension.name, OpdsIdentifierScope::Publication))
    }));
    deduplicate_identifiers(&mut identifiers);

    let mut subjects = entry
        .categories
        .iter()
        .filter_map(|category| {
            let term = nonempty(&category.term)?;
            let name = category.label.as_deref().and_then(nonempty).unwrap_or(term).to_owned();
            Some(OpdsPublicationSubject {
                id: None,
                name,
                name_localizations: Vec::new(),
                sort_name: None,
                sort_name_localizations: Vec::new(),
                source: "opds:category".to_owned(),
                authority: category.scheme.as_deref().and_then(nonempty).map(str::to_owned),
                code: category.scheme.as_deref().and_then(nonempty).map(|_| term.to_owned()),
            })
        })
        .collect::<Vec<_>>();
    subjects.extend(atom_extensions(entry, "subject").filter_map(|extension| {
        let value = nonempty(extension.value.as_deref()?)?.to_owned();
        let authority = atom_extension_attribute(extension, &["authority", "scheme"]).map(str::to_owned);
        let code = atom_extension_attribute(extension, &["code", "term"]).map(str::to_owned);
        Some(OpdsPublicationSubject { id: None, name: value, name_localizations: Vec::new(), sort_name: None, sort_name_localizations: Vec::new(), source: extension.name.clone(), authority, code })
    }));
    deduplicate_subjects(&mut subjects);

    let mut contributors = entry
        .contributors
        .iter()
        .filter_map(|person| {
            let name = nonempty(&person.name)?.to_owned();
            let provider_link = person.uri.as_deref().and_then(|uri| resolve_url(base, uri).ok()).map(|url| OpdsAuthorLink { url: url.to_string(), target: OpdsLinkTarget::Web });
            Some(OpdsContributorCredit { name, name_localizations: Vec::new(), sort_name: None, sort_name_localizations: Vec::new(), identifiers: Vec::new(), provider_link, roles: vec!["contributor".to_owned()] })
        })
        .collect::<Vec<_>>();
    contributors.extend(atom_extensions(entry, "contributor").filter_map(|extension| {
        let name = nonempty(extension.value.as_deref()?)?.to_owned();
        let role = atom_extension_attribute(extension, &["role"]).unwrap_or("contributor").to_owned();
        Some(OpdsContributorCredit { name, name_localizations: Vec::new(), sort_name: None, sort_name_localizations: Vec::new(), identifiers: Vec::new(), provider_link: None, roles: vec![role] })
    }));
    deduplicate_contributors(&mut contributors);

    let mut collections = Vec::new();
    for (local_name, collection_type) in [("series", "series"), ("collection", "collection")] {
        collections.extend(atom_extensions(entry, local_name).filter_map(|extension| {
            let name = nonempty(extension.value.as_deref()?)?.to_owned();
            let sort_name = atom_extension_scalar(extension, &["sortAs", "sort-as", "file-as"]).map(str::to_owned);
            let mut identifiers = atom_extension_values(extension, &["identifier", "altIdentifier", "alternateIdentifier", "id"]);
            deduplicate_strings(&mut identifiers);
            let position = atom_extension_attribute(extension, &["position", "group-position", "series_index", "series-index"]).map(str::to_owned);
            Some(OpdsPublicationCollection {
                name,
                name_localizations: Vec::new(),
                sort_name,
                sort_name_localizations: Vec::new(),
                identifiers,
                kind: CollectionKind::from_label(collection_type).ok(),
                position,
                source: format!("atom:{local_name}"),
            })
        }));
    }

    let published = entry.published.as_ref().map(chrono_date_text);
    let modified = Some(chrono_date_text(&entry.updated));
    let mut dates = Vec::new();
    if let Some(value) = &published {
        dates.push(OpdsPublicationDate { id: None, value: value.clone(), event: Some("published".to_owned()), source: "atom:published".to_owned() });
    }
    if let Some(value) = &modified {
        dates.push(OpdsPublicationDate { id: None, value: value.clone(), event: Some("modified".to_owned()), source: "atom:updated".to_owned() });
    }
    dates.extend(atom_extensions(entry, "date").filter_map(|extension| {
        let value = nonempty(extension.value.as_deref()?)?.to_owned();
        let event = atom_extension_attribute(extension, &["event", "type"]).map(str::to_owned);
        Some(OpdsPublicationDate { id: None, value, event, source: extension.name.clone() })
    }));
    let audiences = atom_extensions(entry, "audience").filter_map(|extension| nonempty(extension.value.as_deref()?).map(|value| OpdsAudience { value: value.to_owned(), source: extension.name.clone() })).collect();
    let age_ranges = atom_extensions(entry, "typicalAgeRange")
        .chain(atom_extensions(entry, "ageRange"))
        .filter_map(|extension| nonempty(extension.value.as_deref()?).map(|value| OpdsAgeRange { value: value.to_owned(), source: extension.name.clone() }))
        .collect();
    let images = atom_publication_images(base, entry);
    let ratings = atom_extensions(entry, "aggregateRating").chain(atom_extensions(entry, "rating")).filter_map(|extension| atom_aggregate_rating(base, extension)).collect();
    let interactions = atom_extensions(entry, "interactionStatistic").filter_map(|extension| atom_interaction_statistic(base, extension)).collect();
    let properties = atom_raw_metadata_properties(base, entry);

    OpdsPublicationMetadata {
        title_localizations: Vec::new(),
        sort_name: None,
        sort_name_localizations: Vec::new(),
        published,
        modified,
        dates,
        publishers,
        identifiers,
        subjects,
        subtitle: atom_extensions(entry, "subtitle").find_map(|extension| extension.value.as_deref().and_then(nonempty).map(str::to_owned)),
        subtitle_localizations: Vec::new(),
        alternate_titles: Vec::new(),
        collections,
        contributors,
        rights: Vec::new(),
        sources: Vec::new(),
        types: Vec::new(),
        formats: Vec::new(),
        relations: Vec::new(),
        coverage: Vec::new(),
        audiences,
        age_ranges,
        images,
        ratings,
        interactions,
        popularity_ranks: Vec::new(),
        properties,
    }
}

fn chrono_date_text(value: &atom_syndication::FixedDateTime) -> String {
    value.to_rfc3339()
}

fn atom_extensions<'a>(entry: &'a AtomEntry, local_name: &'a str) -> impl Iterator<Item = &'a atom_syndication::extension::Extension> {
    entry.extensions.values().flat_map(|namespace| namespace.iter()).filter(move |(name, _)| name.eq_ignore_ascii_case(local_name)).flat_map(|(_, extensions)| extensions.iter())
}

fn atom_extension_attribute<'a>(extension: &'a atom_syndication::extension::Extension, local_names: &[&str]) -> Option<&'a str> {
    extension.attrs.iter().find_map(|(name, value)| {
        let local_name = name.rsplit(':').next().unwrap_or(name);
        local_names.iter().any(|candidate| local_name.eq_ignore_ascii_case(candidate)).then_some(value.as_str()).and_then(nonempty)
    })
}

fn atom_extension_scalar<'a>(extension: &'a atom_syndication::extension::Extension, local_names: &[&str]) -> Option<&'a str> {
    atom_extension_attribute(extension, local_names)
        .or_else(|| extension.children.iter().find(|(name, _)| local_names.iter().any(|candidate| name.eq_ignore_ascii_case(candidate))).and_then(|(_, children)| children.iter().find_map(|child| child.value.as_deref().and_then(nonempty))))
}

fn atom_extension_values(extension: &atom_syndication::extension::Extension, local_names: &[&str]) -> Vec<String> {
    let attributes = extension.attrs.iter().filter_map(|(name, value)| {
        let local_name = name.rsplit(':').next().unwrap_or(name);
        local_names.iter().any(|candidate| local_name.eq_ignore_ascii_case(candidate)).then(|| value.as_str()).and_then(nonempty).map(str::to_owned)
    });
    let children = extension.children.iter().filter(|(name, _)| {
        let local_name = name.rsplit(':').next().unwrap_or(name);
        local_names.iter().any(|candidate| local_name.eq_ignore_ascii_case(candidate))
    });
    attributes.chain(children.flat_map(|(_, children)| children.iter().filter_map(|child| child.value.as_deref().and_then(nonempty).or_else(|| atom_extension_scalar(child, &["value", "identifier"])).map(str::to_owned)))).collect()
}

fn scalar_u64(value: &str) -> Option<u64> {
    value.trim().parse().ok()
}

fn atom_aggregate_rating(base: &Url, extension: &atom_syndication::extension::Extension) -> Option<OpdsAggregateRating> {
    let value = atom_extension_scalar(extension, &["ratingValue", "value"]).or_else(|| extension.value.as_deref().and_then(nonempty))?.to_owned();
    Some(OpdsAggregateRating {
        value,
        rating_count: atom_extension_scalar(extension, &["ratingCount", "count"]).and_then(scalar_u64),
        review_count: atom_extension_scalar(extension, &["reviewCount"]).and_then(scalar_u64),
        best_rating: atom_extension_scalar(extension, &["bestRating"]).map(str::to_owned),
        worst_rating: atom_extension_scalar(extension, &["worstRating"]).map(str::to_owned),
        source: format!("opds1:{base}#{}", extension.name),
    })
}

fn atom_interaction_statistic(base: &Url, extension: &atom_syndication::extension::Extension) -> Option<OpdsInteractionStatistic> {
    Some(OpdsInteractionStatistic {
        interaction_type: atom_extension_scalar(extension, &["interactionType", "type"])?.to_owned(),
        count: atom_extension_scalar(extension, &["userInteractionCount", "count"]).and_then(scalar_u64)?,
        service: atom_extension_scalar(extension, &["interactionService", "service"]).map(str::to_owned),
        source: format!("opds1:{base}#{}", extension.name),
    })
}

fn atom_raw_metadata_properties(base: &Url, entry: &AtomEntry) -> Vec<OpdsPublicationProperty> {
    let mut properties = Vec::new();
    for (namespace, elements) in &entry.extensions {
        for extensions in elements.values() {
            for extension in extensions {
                if properties.len() == MAX_RETAINED_METADATA_PROPERTIES {
                    return properties;
                }
                let Ok(value) = serde_json::to_string(&atom_extension_json(extension)) else { continue };
                if value.len() <= MAX_RETAINED_METADATA_PROPERTY_BYTES {
                    properties.push(OpdsPublicationProperty { id: None, property: extension.name.clone(), value, refines: None, scheme: None, language: None, source: format!("opds1:{base}#{namespace}") });
                }
            }
        }
    }
    properties
}

fn atom_extension_json(extension: &atom_syndication::extension::Extension) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("name".to_owned(), Value::String(extension.name.clone()));
    if let Some(value) = &extension.value {
        object.insert("value".to_owned(), Value::String(value.clone()));
    }
    if !extension.attrs.is_empty() {
        object.insert("attributes".to_owned(), Value::Object(extension.attrs.iter().map(|(name, value)| (name.clone(), Value::String(value.clone()))).collect()));
    }
    if !extension.children.is_empty() {
        object.insert("children".to_owned(), Value::Object(extension.children.iter().map(|(name, children)| (name.clone(), Value::Array(children.iter().map(atom_extension_json).collect()))).collect()));
    }
    Value::Object(object)
}

fn json_publication_metadata(base: &Url, metadata: &serde_json::Map<String, Value>) -> OpdsPublicationMetadata {
    let publishers = metadata.get("publisher").map(|value| contributor_credits(base, value).into_iter().map(opds_agent_credit).collect()).unwrap_or_default();
    let identifiers = metadata.get("identifier").map(|value| json_publication_identifiers(value, "opds2:metadata.identifier", OpdsIdentifierScope::Publication)).unwrap_or_default();
    let subjects = metadata.get("subject").map(json_subjects).unwrap_or_default();
    let collections = metadata.get("belongsTo").and_then(Value::as_object).map(|belongs_to| belongs_to.iter().flat_map(|(collection_type, value)| json_collections(value, collection_type)).collect()).unwrap_or_default();
    let contributor_fields = ["contributor", "translator", "editor", "artist", "illustrator", "letterer", "penciler", "colorist", "inker", "narrator"];
    let contributors = contributor_fields.into_iter().flat_map(|role| metadata.get(role).into_iter().flat_map(move |value| json_contributor_credits(base, value, role))).collect::<Vec<_>>();
    let published = metadata.get("published").and_then(Value::as_str).and_then(nonempty).map(str::to_owned);
    let modified = metadata.get("modified").and_then(Value::as_str).and_then(nonempty).map(str::to_owned);
    let mut dates = Vec::new();
    if let Some(value) = &published {
        dates.push(OpdsPublicationDate { id: None, value: value.clone(), event: Some("published".to_owned()), source: "opds2:metadata.published".to_owned() });
    }
    if let Some(value) = &modified {
        dates.push(OpdsPublicationDate { id: None, value: value.clone(), event: Some("modified".to_owned()), source: "opds2:metadata.modified".to_owned() });
    }
    let audiences = metadata.get("audience").map(|value| sourced_json_texts(value, "opds2:metadata.audience").into_iter().map(|(value, source)| OpdsAudience { value, source }).collect()).unwrap_or_default();
    let age_ranges = metadata
        .get("typicalAgeRange")
        .or_else(|| metadata.get("ageRange"))
        .map(|value| sourced_json_texts(value, "opds2:metadata.typicalAgeRange").into_iter().map(|(value, source)| OpdsAgeRange { value, source }).collect())
        .unwrap_or_default();
    let ratings = metadata.get("aggregateRating").map(|value| json_aggregate_ratings(base, value)).unwrap_or_default();
    let interactions = metadata.get("interactionStatistic").map(|value| json_interaction_statistics(base, value)).unwrap_or_default();
    let properties = json_raw_metadata_properties(base, metadata);
    OpdsPublicationMetadata {
        title_localizations: metadata.get("title").map(localized_texts).unwrap_or_default(),
        sort_name: metadata.get("sortAs").and_then(localized_text).and_then(|value| nonempty(&value).map(str::to_owned)),
        sort_name_localizations: metadata.get("sortAs").map(localized_texts).unwrap_or_default(),
        published,
        modified,
        dates,
        publishers,
        identifiers,
        subjects,
        subtitle: metadata.get("subtitle").and_then(localized_text).and_then(|value| nonempty(&value).map(str::to_owned)),
        subtitle_localizations: metadata.get("subtitle").map(localized_texts).unwrap_or_default(),
        alternate_titles: Vec::new(),
        collections,
        contributors,
        rights: Vec::new(),
        sources: Vec::new(),
        types: Vec::new(),
        formats: Vec::new(),
        relations: Vec::new(),
        coverage: Vec::new(),
        audiences,
        age_ranges,
        images: Vec::new(),
        ratings,
        interactions,
        popularity_ranks: Vec::new(),
        properties,
    }
}

fn json_numeric_text(value: &Value) -> Option<String> {
    match value {
        Value::Number(value) => Some(value.to_string()),
        Value::String(value) => nonempty(value).map(str::to_owned),
        _ => None,
    }
}

fn json_u64(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str().and_then(scalar_u64))
}

fn json_aggregate_ratings(base: &Url, value: &Value) -> Vec<OpdsAggregateRating> {
    match value {
        Value::Array(values) => values.iter().flat_map(|value| json_aggregate_ratings(base, value)).collect(),
        Value::Object(rating) => {
            let Some(value) = rating.get("ratingValue").or_else(|| rating.get("value")).and_then(json_numeric_text) else { return Vec::new() };
            vec![OpdsAggregateRating {
                value,
                rating_count: rating.get("ratingCount").and_then(json_u64),
                review_count: rating.get("reviewCount").and_then(json_u64),
                best_rating: rating.get("bestRating").and_then(json_numeric_text),
                worst_rating: rating.get("worstRating").and_then(json_numeric_text),
                source: format!("opds2:{base}#metadata.aggregateRating"),
            }]
        }
        _ => Vec::new(),
    }
}

fn json_object_label(value: &Value) -> Option<String> {
    value.as_str().and_then(nonempty).map(str::to_owned).or_else(|| {
        let object = value.as_object()?;
        object.get("@id").or_else(|| object.get("id")).or_else(|| object.get("url")).or_else(|| object.get("name")).and_then(localized_text).and_then(|value| nonempty(&value).map(str::to_owned))
    })
}

fn json_interaction_statistics(base: &Url, value: &Value) -> Vec<OpdsInteractionStatistic> {
    match value {
        Value::Array(values) => values.iter().flat_map(|value| json_interaction_statistics(base, value)).collect(),
        Value::Object(interaction) => {
            let Some(interaction_type) = interaction.get("interactionType").and_then(json_object_label) else { return Vec::new() };
            let Some(count) = interaction.get("userInteractionCount").or_else(|| interaction.get("count")).and_then(json_u64) else { return Vec::new() };
            vec![OpdsInteractionStatistic { interaction_type, count, service: interaction.get("interactionService").and_then(json_object_label), source: format!("opds2:{base}#metadata.interactionStatistic") }]
        }
        _ => Vec::new(),
    }
}

fn json_raw_metadata_properties(base: &Url, metadata: &serde_json::Map<String, Value>) -> Vec<OpdsPublicationProperty> {
    metadata
        .iter()
        .take(MAX_RETAINED_METADATA_PROPERTIES)
        .filter_map(|(property, raw)| {
            let value = serde_json::to_string(raw).ok()?;
            (value.len() <= MAX_RETAINED_METADATA_PROPERTY_BYTES).then(|| OpdsPublicationProperty { id: None, property: property.clone(), value, refines: None, scheme: None, language: None, source: format!("opds2:{base}#metadata") })
        })
        .collect()
}

fn json_publication_identifiers(value: &Value, source: &str, scope: OpdsIdentifierScope) -> Vec<OpdsPublicationIdentifier> {
    match value {
        Value::String(value) => nonempty(value).map(|value| vec![opds_identifier(value, None, source, scope)]).unwrap_or_default(),
        Value::Array(values) => values.iter().flat_map(|value| json_publication_identifiers(value, source, scope)).collect(),
        Value::Object(identifier) => {
            let Some(value) = identifier.get("value").or_else(|| identifier.get("identifier")).and_then(Value::as_str).and_then(nonempty) else { return Vec::new() };
            let scheme = identifier.get("scheme").or_else(|| identifier.get("type")).and_then(Value::as_str).and_then(nonempty).map(str::to_owned);
            vec![opds_identifier(value, scheme, source, scope)]
        }
        _ => Vec::new(),
    }
}

fn opds_identifier(value: &str, scheme: Option<String>, source: &str, scope: OpdsIdentifierScope) -> OpdsPublicationIdentifier {
    let scheme = scheme.or_else(|| identifier_scheme_from_value(value));
    OpdsPublicationIdentifier { value: value.to_owned(), scheme, source: source.to_owned(), scope }
}

fn append_identity_evidence(metadata: &mut OpdsPublicationMetadata, id: &str, work_id: Option<&str>, source: &str) {
    metadata.identifiers.push(opds_identifier(id, None, source, OpdsIdentifierScope::CatalogEntry));
    if let Some(work_id) = work_id {
        metadata.identifiers.push(opds_identifier(work_id, None, source, OpdsIdentifierScope::Work));
    }
    deduplicate_identifiers(&mut metadata.identifiers);
}

fn identifier_scheme_from_value(value: &str) -> Option<String> {
    let lower = value.trim().to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("urn:") {
        return rest.split(':').next().and_then(nonempty).map(str::to_owned);
    }
    for scheme in ["isbn", "doi", "oclc", "lccn", "issn", "asin", "uuid"] {
        if lower.strip_prefix(scheme).is_some_and(|rest| rest.starts_with(':')) {
            return Some(scheme.to_owned());
        }
    }
    None
}

fn sourced_json_texts(value: &Value, source: &str) -> Vec<(String, String)> {
    match value {
        Value::String(value) => nonempty(value).map(|value| vec![(value.to_owned(), source.to_owned())]).unwrap_or_default(),
        Value::Number(value) => vec![(value.to_string(), source.to_owned())],
        Value::Array(values) => values.iter().flat_map(|value| sourced_json_texts(value, source)).collect(),
        Value::Object(value) => value
            .get("name")
            .or_else(|| value.get("value"))
            .or_else(|| value.get("identifier"))
            .and_then(localized_text)
            .and_then(|value| nonempty(&value).map(str::to_owned))
            .map(|value| vec![(value, source.to_owned())])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn opds_agent_credit(credit: OpdsAuthorCredit) -> OpdsAgentCredit {
    OpdsAgentCredit {
        name: credit.name,
        name_localizations: credit.name_localizations,
        sort_name: credit.sort_name,
        sort_name_localizations: credit.sort_name_localizations,
        identifiers: credit.identifiers,
        provider_link: credit.provider_link,
    }
}

fn json_contributor_credits(base: &Url, value: &Value, default_role: &str) -> Vec<OpdsContributorCredit> {
    match value {
        Value::Array(values) => values.iter().flat_map(|value| json_contributor_credits(base, value, default_role)).collect(),
        _ => contributor_credits(base, value)
            .into_iter()
            .map(|credit| {
                let mut roles = value.get("role").map(identifier_values).unwrap_or_default();
                if roles.is_empty() {
                    roles.push(default_role.to_owned());
                }
                OpdsContributorCredit {
                    name: credit.name,
                    name_localizations: credit.name_localizations,
                    sort_name: credit.sort_name,
                    sort_name_localizations: credit.sort_name_localizations,
                    identifiers: credit.identifiers,
                    provider_link: credit.provider_link,
                    roles,
                }
            })
            .collect(),
    }
}

fn json_subjects(value: &Value) -> Vec<OpdsPublicationSubject> {
    match value {
        Value::String(name) => nonempty(name)
            .map(|name| vec![OpdsPublicationSubject { id: None, name: name.to_owned(), name_localizations: Vec::new(), sort_name: None, sort_name_localizations: Vec::new(), source: "opds2:subject".to_owned(), authority: None, code: None }])
            .unwrap_or_default(),
        Value::Object(subject) => {
            let code = subject.get("code").and_then(Value::as_str).and_then(nonempty).map(str::to_owned);
            let name = subject.get("name").and_then(localized_text).and_then(|name| nonempty(&name).map(str::to_owned)).or_else(|| code.clone());
            let name_localizations = subject.get("name").map(localized_texts).unwrap_or_default();
            let sort_name = subject.get("sortAs").and_then(localized_text).and_then(|name| nonempty(&name).map(str::to_owned));
            let sort_name_localizations = subject.get("sortAs").map(localized_texts).unwrap_or_default();
            let authority = subject.get("scheme").or_else(|| subject.get("authority")).and_then(Value::as_str).and_then(nonempty).map(str::to_owned);
            name.map(|name| vec![OpdsPublicationSubject { id: None, name, name_localizations, sort_name, sort_name_localizations, source: "opds2:subject".to_owned(), authority, code }]).unwrap_or_default()
        }
        Value::Array(subjects) => subjects.iter().flat_map(json_subjects).collect(),
        _ => Vec::new(),
    }
}

fn json_collections(value: &Value, collection_type: &str) -> Vec<OpdsPublicationCollection> {
    match value {
        Value::String(name) => nonempty(name)
            .map(|name| {
                vec![OpdsPublicationCollection {
                    name: name.to_owned(),
                    name_localizations: Vec::new(),
                    sort_name: None,
                    sort_name_localizations: Vec::new(),
                    identifiers: Vec::new(),
                    kind: CollectionKind::from_label(collection_type).ok(),
                    position: None,
                    source: format!("opds2:metadata.belongsTo.{collection_type}"),
                }]
            })
            .unwrap_or_default(),
        Value::Object(collection) => {
            let name = collection.get("name").or_else(|| collection.get("title")).and_then(localized_text).and_then(|name| nonempty(&name).map(str::to_owned));
            let name_localizations = collection.get("name").or_else(|| collection.get("title")).map(localized_texts).unwrap_or_default();
            let sort_name = collection.get("sortAs").and_then(localized_text).and_then(|name| nonempty(&name).map(str::to_owned));
            let sort_name_localizations = collection.get("sortAs").map(localized_texts).unwrap_or_default();
            let mut identifiers = collection.get("identifier").map(identifier_values).unwrap_or_default();
            identifiers.extend(collection.get("altIdentifier").map(identifier_values).unwrap_or_default());
            identifiers.extend(collection.get("alternateIdentifier").map(identifier_values).unwrap_or_default());
            deduplicate_strings(&mut identifiers);
            let position = collection.get("position").and_then(json_scalar_text);
            name.map(|name| {
                vec![OpdsPublicationCollection {
                    name,
                    name_localizations,
                    sort_name,
                    sort_name_localizations,
                    identifiers,
                    kind: CollectionKind::from_label(collection_type).ok(),
                    position,
                    source: format!("opds2:metadata.belongsTo.{collection_type}"),
                }]
            })
            .unwrap_or_default()
        }
        Value::Array(collections) => collections.iter().flat_map(|collection| json_collections(collection, collection_type)).collect(),
        _ => Vec::new(),
    }
}

fn json_scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => nonempty(value).map(str::to_owned),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn atom_publication_images(base: &Url, entry: &AtomEntry) -> Vec<OpdsPublicationImage> {
    let mut images = entry
        .links
        .iter()
        .filter(|link| is_image_relation(&link.rel))
        .filter_map(|link| {
            let url = resolve_url(base, &link.href).ok()?.to_string();
            Some(OpdsPublicationImage { url, media_type: link.mime_type.clone(), relations: vec![link.rel.clone()], width: None, height: None, source: "opds1:link".to_owned() })
        })
        .collect::<Vec<_>>();
    deduplicate_images(&mut images);
    images
}

fn json_publication_images(base: &Url, publication: &serde_json::Map<String, Value>) -> Vec<OpdsPublicationImage> {
    let mut images = Vec::new();
    if let Some(values) = publication.get("images").and_then(Value::as_array) {
        images.extend(values.iter().filter_map(|value| json_publication_image(base, value, "opds2:images")));
    }
    if let Some(values) = publication.get("links").and_then(Value::as_array) {
        images.extend(values.iter().filter(|value| json_relations(value).any(is_image_relation)).filter_map(|value| json_publication_image(base, value, "opds2:links")));
    }
    deduplicate_images(&mut images);
    images
}

fn json_publication_image(base: &Url, value: &Value, source: &str) -> Option<OpdsPublicationImage> {
    let object = value.as_object()?;
    Some(OpdsPublicationImage {
        url: json_link_url(base, value)?.to_string(),
        media_type: object.get("type").and_then(Value::as_str).and_then(nonempty).map(str::to_owned),
        relations: json_relations(value).map(str::to_owned).collect(),
        width: object.get("width").and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok()),
        height: object.get("height").and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok()),
        source: source.to_owned(),
    })
}

fn preferred_image_url(images: &[OpdsPublicationImage]) -> Option<String> {
    images.iter().find(|image| !image.relations.iter().any(|relation| relation == "thumbnail" || relation.ends_with("/thumbnail"))).or_else(|| images.first()).map(|image| image.url.clone())
}

fn deduplicate_identifiers(identifiers: &mut Vec<OpdsPublicationIdentifier>) {
    let mut seen = HashSet::new();
    identifiers.retain(|identifier| seen.insert((identifier.value.clone(), identifier.scheme.clone(), identifier.source.clone(), identifier.scope)));
}

fn deduplicate_images(images: &mut Vec<OpdsPublicationImage>) {
    let mut seen = HashSet::new();
    images.retain(|image| seen.insert((image.url.clone(), image.media_type.clone(), image.relations.clone(), image.width, image.height, image.source.clone())));
}

fn deduplicate_subjects(subjects: &mut Vec<OpdsPublicationSubject>) {
    let mut seen = HashSet::new();
    subjects.retain(|subject| seen.insert((subject.name.clone(), subject.source.clone(), subject.authority.clone(), subject.code.clone())));
}

fn deduplicate_contributors(contributors: &mut Vec<OpdsContributorCredit>) {
    let mut seen = HashSet::new();
    contributors.retain(|contributor| seen.insert((contributor.name.clone(), contributor.roles.clone())));
}

fn deduplicate_strings(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn atom_publication_languages(entry: &atom_syndication::Entry) -> Vec<String> {
    let values = entry.extensions.values().flat_map(|namespace| namespace.iter()).filter(|(name, _)| name.eq_ignore_ascii_case("language")).flat_map(|(_, extensions)| extensions).filter_map(|extension| extension.value.as_deref());
    canonical_language_tags(values)
}

fn publication_languages(value: &Value) -> Vec<String> {
    match value {
        Value::String(value) => canonical_language_tags([value.as_str()]),
        Value::Array(values) => canonical_language_tags(values.iter().filter_map(Value::as_str)),
        _ => Vec::new(),
    }
}

fn canonical_language_tags<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut languages = values.into_iter().filter_map(canonical_language_tag).collect::<Vec<_>>();
    languages.sort();
    languages.dedup();
    languages
}

fn canonical_language_tag(value: &str) -> Option<String> {
    OpdsLanguageTag::parse(value).ok().map(OpdsLanguageTag::into_string)
}

fn stable_publication_id(explicit: Option<&str>, self_url: Option<&str>, acquisitions: &[OpdsAcquisition], base: &Url, title: &str, authors: &[OpdsAuthorCredit]) -> String {
    let acquisition_url = acquisitions.first().map(|acquisition| match acquisition {
        OpdsAcquisition::Direct(acquisition) => acquisition.url.as_str(),
        OpdsAcquisition::Indirect(acquisition) => acquisition.url.as_str(),
        OpdsAcquisition::Unsupported(acquisition) => acquisition.url.as_str(),
    });
    if let Some(candidate) = [explicit.and_then(nonempty), self_url.and_then(nonempty), acquisition_url.and_then(nonempty)].into_iter().flatten().find(|candidate| valid_catalog_identity_text(candidate)) {
        return candidate.to_owned();
    }
    let mut hash = blake3::Hasher::new();
    let origin = base.origin().ascii_serialization();
    for value in std::iter::once(origin.as_str()).chain(std::iter::once(title)).chain(authors.iter().map(|author| author.name.as_str())) {
        hash.update(value.as_bytes());
        hash.update(&[0]);
    }
    format!("urn:bokheim:opds:synthetic:{}", hash.finalize().to_hex())
}

fn work_id_from_publication_id(id: &str) -> Option<String> {
    let rest = id.strip_prefix("urn:gutenberg:")?;
    let number = rest.split(':').next()?;
    (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())).then(|| format!("urn:gutenberg:{number}"))
}

fn atom_description(entry: &AtomEntry) -> Option<OpdsDescription> {
    if let Some(summary) = &entry.summary {
        let format = match summary.r#type {
            TextType::Text => DescriptionFormat::PlainText,
            TextType::Html => DescriptionFormat::Html,
            TextType::Xhtml => DescriptionFormat::Xhtml,
        };
        return typed_description(summary.as_str(), format);
    }
    let content = entry.content.as_ref()?;
    let value = content.value()?;
    let content_type = content.content_type.as_deref().unwrap_or("text").split(';').next().unwrap_or("text").trim().to_ascii_lowercase();
    let format = match content_type.as_str() {
        "html" | "text/html" => DescriptionFormat::Html,
        "xhtml" | "application/xhtml+xml" => DescriptionFormat::Xhtml,
        _ => DescriptionFormat::PlainText,
    };
    typed_description(value, format)
}

fn normalize_project_gutenberg_description(id: &str, description: OpdsDescription) -> Option<OpdsDescription> {
    if !id.starts_with("urn:gutenberg:") {
        return Some(description);
    }
    match description {
        OpdsDescription::PlainText { text } => compact_description_whitespace(&strip_project_gutenberg_plain_text(&text)).map(|text| OpdsDescription::PlainText { text }),
        description @ (OpdsDescription::Html { .. } | OpdsDescription::Xhtml { .. }) => Some(description),
    }
}

pub fn strip_project_gutenberg_plain_text(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if let Some(summary) = lower.find("summary:") {
        return value[summary + "summary:".len()..].trim().to_owned();
    }
    value.lines().filter(|line| !is_project_gutenberg_boilerplate(line)).collect::<Vec<_>>().join("\n")
}

fn is_project_gutenberg_boilerplate(value: &str) -> bool {
    let text = value.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
    matches!(text.as_str(), "this edition has images." | "this edition had all images removed.")
        || text.starts_with("title:")
        || text.starts_with("credits:")
        || text.starts_with("note: wikipedia page about this book:")
        || (text.starts_with("note: this title is also available as ebook") || text.starts_with("this title is also available as ebook"))
}

#[derive(Clone, Copy)]
enum DescriptionFormat {
    PlainText,
    Html,
    Xhtml,
}

fn inferred_description(value: &str) -> Option<OpdsDescription> {
    let format = if value.trim_start().starts_with('<') { DescriptionFormat::Html } else { DescriptionFormat::PlainText };
    typed_description(value, format)
}

fn typed_description(value: &str, format: DescriptionFormat) -> Option<OpdsDescription> {
    let value = nonempty(value)?;
    match format {
        DescriptionFormat::PlainText => compact_description_whitespace(value).map(|text| OpdsDescription::PlainText { text }),
        DescriptionFormat::Html => Some(OpdsDescription::Html { markup: value.to_owned() }),
        DescriptionFormat::Xhtml => Some(OpdsDescription::Xhtml { markup: value.to_owned() }),
    }
}

fn compact_description_whitespace(value: &str) -> Option<String> {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!compact.is_empty()).then_some(compact)
}

fn preferred_download(acquisitions: &[OpdsAcquisition]) -> Option<OpdsDirectAcquisition> {
    acquisitions.iter().find_map(|acquisition| match acquisition {
        OpdsAcquisition::Direct(direct) if is_importable_acquisition(direct) => Some(direct.clone()),
        _ => None,
    })
}

fn is_importable_acquisition(acquisition: &OpdsDirectAcquisition) -> bool {
    acquisition.price.is_none() && matches!(acquisition.kind, OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic)
}

fn contributor_credits(base: &Url, value: &Value) -> Vec<OpdsAuthorCredit> {
    match value {
        Value::String(name) => {
            nonempty(name).map(|name| vec![OpdsAuthorCredit { name: name.to_owned(), name_localizations: Vec::new(), sort_name: None, sort_name_localizations: Vec::new(), identifiers: Vec::new(), provider_link: None }]).unwrap_or_default()
        }
        Value::Object(author) => {
            let Some(name) = author.get("name").and_then(localized_text).and_then(|name| nonempty(&name).map(str::to_owned)) else {
                return Vec::new();
            };
            let name_localizations = author.get("name").map(localized_texts).unwrap_or_default();
            let sort_name = author.get("sortAs").and_then(localized_text).and_then(|name| nonempty(&name).map(str::to_owned));
            let sort_name_localizations = author.get("sortAs").map(localized_texts).unwrap_or_default();
            let identifiers = author.get("identifier").map(identifier_values).unwrap_or_default();
            let provider_link = author.get("links").and_then(Value::as_array).and_then(|links| links.iter().find_map(|link| contributor_link(base, link)));
            vec![OpdsAuthorCredit { name, name_localizations, sort_name, sort_name_localizations, identifiers, provider_link }]
        }
        Value::Array(authors) => authors.iter().flat_map(|author| contributor_credits(base, author)).collect(),
        _ => Vec::new(),
    }
}

fn identifier_values(value: &Value) -> Vec<String> {
    match value {
        Value::String(identifier) => nonempty(identifier).map(|identifier| vec![identifier.to_owned()]).unwrap_or_default(),
        Value::Array(identifiers) => identifiers.iter().flat_map(identifier_values).collect(),
        Value::Object(identifier) => identifier.get("value").or_else(|| identifier.get("identifier")).or_else(|| identifier.get("@id")).or_else(|| identifier.get("id")).map(identifier_values).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn contributor_link(base: &Url, value: &Value) -> Option<OpdsAuthorLink> {
    let url = json_link_url(base, value)?.to_string();
    let target = value.get("type").and_then(Value::as_str).map(link_target_from_media_type).unwrap_or(OpdsLinkTarget::Web);
    Some(OpdsAuthorLink { url, target })
}

fn localized_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Object(variants) => variants.get("en").and_then(Value::as_str).or_else(|| variants.get("und").and_then(Value::as_str)).or_else(|| variants.values().find_map(Value::as_str)).map(str::to_owned),
        _ => None,
    }
}

fn localized_texts(value: &Value) -> Vec<OpdsLocalizedText> {
    let Value::Object(variants) = value else { return Vec::new() };
    let mut localized = variants
        .iter()
        .filter_map(|(language, value)| {
            let language = canonical_language_tag(language)?;
            let value = value.as_str().and_then(nonempty)?.to_owned();
            Some(OpdsLocalizedText { language, value })
        })
        .collect::<Vec<_>>();
    localized.sort_by(|left, right| left.language.cmp(&right.language).then_with(|| left.value.cmp(&right.value)));
    localized.dedup();
    localized
}

#[derive(Default)]
struct Opds1AcquisitionDetails {
    price: Option<OpdsPrice>,
    available_formats: Vec<OpdsBookFormat>,
}

fn parse_opds_1_acquisition_details(bytes: &[u8]) -> Result<Vec<Vec<Opds1AcquisitionDetails>>, OpdsError> {
    let mut reader = Reader::from_reader(bytes);
    let mut entries = Vec::<Vec<Opds1AcquisitionDetails>>::new();
    let mut entry_index = None;
    let mut current = None::<Opds1AcquisitionDetails>;
    let mut price_currency = None::<String>;
    let mut price_value = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) if element.local_name().as_ref() == b"entry" => {
                entries.push(Vec::new());
                entry_index = Some(entries.len() - 1);
            }
            Ok(Event::End(element)) if element.local_name().as_ref() == b"entry" => entry_index = None,
            Ok(Event::Start(element)) if element.local_name().as_ref() == b"link" && entry_index.is_some() => {
                if xml_attribute(&reader, &element, b"rel")?.as_deref().and_then(acquisition_kind).is_some() {
                    current = Some(Opds1AcquisitionDetails::default());
                }
            }
            Ok(Event::Empty(element)) if element.local_name().as_ref() == b"link" && entry_index.is_some() => {
                if xml_attribute(&reader, &element, b"rel")?.as_deref().and_then(acquisition_kind).is_some() {
                    entries[entry_index.expect("entry presence checked")].push(Opds1AcquisitionDetails::default());
                }
            }
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) if element.local_name().as_ref() == b"indirectAcquisition" && current.is_some() => {
                if let Some(book_format) = xml_attribute(&reader, &element, b"type")?.as_deref().and_then(book_format_from_media_type) {
                    let formats = &mut current.as_mut().expect("acquisition presence checked").available_formats;
                    if !formats.contains(&book_format) {
                        formats.push(book_format);
                    }
                }
            }
            Ok(Event::Start(element)) if element.local_name().as_ref() == b"price" && current.is_some() => {
                price_currency = xml_attribute(&reader, &element, b"currencycode")?.or(xml_attribute(&reader, &element, b"currency")?);
                price_value.clear();
            }
            Ok(Event::Text(text)) if price_currency.is_some() => price_value.push_str(&String::from_utf8_lossy(text.as_ref())),
            Ok(Event::End(element)) if element.local_name().as_ref() == b"price" && current.is_some() => {
                if let (Some(currency), Some(value)) = (price_currency.take(), nonempty(&price_value)) {
                    current.as_mut().expect("acquisition presence checked").price = Some(OpdsPrice { currency, value: value.to_owned() });
                }
            }
            Ok(Event::End(element)) if element.local_name().as_ref() == b"link" && current.is_some() => {
                if let Some(index) = entry_index {
                    entries[index].push(current.take().expect("acquisition presence checked"));
                }
                price_currency = None;
                price_value.clear();
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(OpdsError::InvalidCatalog(error.to_string())),
        }
    }
    Ok(entries)
}

fn xml_attribute(reader: &Reader<&[u8]>, element: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Result<Option<String>, OpdsError> {
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(|error| OpdsError::InvalidCatalog(error.to_string()))?;
        if attribute.key.local_name().as_ref() == name {
            return attribute.decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder()).map(|value| Some(value.into_owned())).map_err(|error| OpdsError::InvalidCatalog(error.to_string()));
        }
    }
    Ok(None)
}

fn acquisition_from_atom(base: &Url, link: &AtomLink, title: &str, details: Option<&Opds1AcquisitionDetails>) -> Option<OpdsAcquisition> {
    let kind = acquisition_kind(&link.rel)?;
    let url = resolve_url(base, &link.href).ok()?;
    Some(acquisition(url, kind, link.mime_type.as_deref(), title, details.and_then(|details| details.price.clone()), details.map(|details| details.available_formats.clone()).unwrap_or_default()))
}

fn acquisition_from_json(base: &Url, value: &Value, title: &str) -> Option<OpdsAcquisition> {
    let object = value.as_object()?;
    let kind = json_relations(value).find_map(acquisition_kind)?;
    let url = resolve_url(base, object.get("href")?.as_str()?).ok()?;
    let properties = object.get("properties").and_then(Value::as_object);
    let price = properties.and_then(|properties| properties.get("price")).and_then(price_from_json);
    let available_formats = properties.and_then(|properties| properties.get("indirectAcquisition")).map(indirect_formats_from_json).unwrap_or_default();
    Some(acquisition(url, kind, object.get("type").and_then(Value::as_str), title, price, available_formats))
}

fn acquisition(url: Url, kind: OpdsAcquisitionKind, media_type: Option<&str>, title: &str, price: Option<OpdsPrice>, available_formats: Vec<OpdsBookFormat>) -> OpdsAcquisition {
    if let Some(book_format) = media_type.and_then(book_format_from_media_type).or_else(|| book_format_from_url(&url)) {
        OpdsAcquisition::Direct(OpdsDirectAcquisition { suggested_file_name: safe_book_file_name(title, &url, book_format), url: url.to_string(), kind, book_format, price })
    } else if !available_formats.is_empty() {
        OpdsAcquisition::Indirect(OpdsIndirectAcquisition { url: url.to_string(), kind, media_type: media_type.map(str::to_owned), available_formats, price })
    } else {
        OpdsAcquisition::Unsupported(OpdsUnsupportedAcquisition { url: url.to_string(), kind, media_type: media_type.map(str::to_owned), price })
    }
}

fn price_from_json(value: &Value) -> Option<OpdsPrice> {
    let object = value.as_object()?;
    let currency = nonempty(object.get("currency")?.as_str()?)?.to_owned();
    let value = match object.get("value")? {
        Value::String(value) => nonempty(value)?.to_owned(),
        Value::Number(value) => value.to_string(),
        _ => return None,
    };
    Some(OpdsPrice { currency, value })
}

fn indirect_formats_from_json(value: &Value) -> Vec<OpdsBookFormat> {
    fn visit(value: &Value, formats: &mut Vec<OpdsBookFormat>) {
        let Some(object) = value.as_object() else { return };
        if let Some(book_format) = object.get("type").and_then(Value::as_str).and_then(book_format_from_media_type) {
            if !formats.contains(&book_format) {
                formats.push(book_format);
            }
        }
        if let Some(children) = object.get("child").and_then(Value::as_array) {
            for child in children {
                visit(child, formats);
            }
        }
    }
    let mut formats = Vec::new();
    if let Some(values) = value.as_array() {
        for value in values {
            visit(value, &mut formats);
        }
    }
    formats
}

fn acquisition_kind(relation: &str) -> Option<OpdsAcquisitionKind> {
    match relation {
        "download" | "http://opds-spec.org/acquisition/open-access" => Some(OpdsAcquisitionKind::OpenAccess),
        "borrow" | "http://opds-spec.org/acquisition/borrow" => Some(OpdsAcquisitionKind::Borrow),
        "buy" | "http://opds-spec.org/acquisition/buy" => Some(OpdsAcquisitionKind::Buy),
        "preview" | "http://opds-spec.org/acquisition/sample" => Some(OpdsAcquisitionKind::Preview),
        "subscribe" | "http://opds-spec.org/acquisition/subscribe" => Some(OpdsAcquisitionKind::Subscribe),
        "acquisition" | "http://opds-spec.org/acquisition" => Some(OpdsAcquisitionKind::Generic),
        _ => None,
    }
}

fn catalog_link_from_json(base: &Url, value: &Value) -> Option<OpdsCatalogLink> {
    let object = value.as_object()?;
    let url = resolve_url(base, object.get("href")?.as_str()?).ok()?;
    let title = object.get("title").and_then(localized_text).unwrap_or_else(|| "Untitled".to_owned());
    let target = object.get("type").and_then(Value::as_str).map(link_target_from_media_type).unwrap_or(OpdsLinkTarget::Catalog);
    let number_of_items = object.get("properties").and_then(Value::as_object).and_then(|properties| properties.get("numberOfItems")).and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok());
    let active = json_link_has_relation(value, "self");
    Some(OpdsCatalogLink { title, url: url.to_string(), target, relations: json_relations(value).map(str::to_owned).collect(), number_of_items, active })
}

fn relation_url_atom(base: &Url, links: &[AtomLink], relation: &str) -> Option<String> {
    links.iter().find(|link| link.rel == relation).and_then(|link| resolve_url(base, &link.href).ok()).map(|url| url.to_string())
}

fn relation_url_json(base: &Url, links: &[Value], relation: &str) -> Option<String> {
    links.iter().find(|link| json_link_has_relation(link, relation)).and_then(|link| json_link_url(base, link)).map(|url| url.to_string())
}

fn json_link_url(base: &Url, value: &Value) -> Option<Url> {
    resolve_url(base, value.as_object()?.get("href")?.as_str()?).ok()
}

fn json_relations(value: &Value) -> impl Iterator<Item = &str> {
    value.as_object().and_then(|object| object.get("rel")).into_iter().flat_map(|relations| match relations {
        Value::String(relation) => vec![relation.as_str()],
        Value::Array(relations) => relations.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    })
}

fn json_link_has_relation(value: &Value, relation: &str) -> bool {
    json_relations(value).any(|candidate| candidate == relation)
}

fn parse_http_url(value: &str) -> Result<Url, OpdsError> {
    let url = Url::parse(value).map_err(|error| OpdsError::InvalidUrl(error.to_string()))?;
    validate_http_url(url)
}

fn looks_like_html(bytes: &[u8]) -> bool {
    let prefix = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    prefix.contains("<!doctype html") || prefix.contains("<html")
}

fn looks_like_atom(bytes: &[u8]) -> bool {
    let prefix = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_ascii_lowercase();
    (prefix.contains("<feed") || prefix.contains("<entry")) && (prefix.contains("w3.org/2005/atom") || prefix.contains("opds"))
}

fn discover_opds_url(base: &Url, link_headers: &[String], bytes: &[u8]) -> Option<Url> {
    for header in link_headers {
        for link in header.split(',') {
            let Some(target) = link.trim().strip_prefix('<').and_then(|link| link.split_once('>').map(|(target, _)| target)) else { continue };
            if link.to_ascii_lowercase().contains("opds") && (link.to_ascii_lowercase().contains("rel=\"alternate\"") || link.to_ascii_lowercase().contains("rel=alternate")) {
                if let Ok(url) = resolve_url(base, target) {
                    return Some(url);
                }
            }
        }
    }
    let mut reader = Reader::from_reader(bytes);
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) if element.local_name().as_ref().eq_ignore_ascii_case(b"link") => {
                let rel = xml_attribute(&reader, &element, b"rel").ok().flatten().unwrap_or_default().to_ascii_lowercase();
                let media_type = xml_attribute(&reader, &element, b"type").ok().flatten().unwrap_or_default();
                if rel.split_ascii_whitespace().any(|relation| relation == "alternate") && is_catalog_media_type(&media_type) {
                    if let Some(href) = xml_attribute(&reader, &element, b"href").ok().flatten() {
                        if let Ok(url) = resolve_url(base, &href) {
                            return Some(url);
                        }
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    None
}

fn resolve_url(base: &Url, value: &str) -> Result<Url, OpdsError> {
    let mut url = base.join(value).map_err(|error| OpdsError::InvalidUrl(error.to_string()))?;
    // Some established OPDS feeds (notably OpenEdition) are served over
    // HTTPS but still emit absolute HTTP links back to their own host. Never
    // follow the downgrade: canonicalize only that same-host/default-port
    // case to the already authenticated HTTPS origin. Cross-host links and
    // non-default ports remain untouched and are still subject to the
    // catalogue origin policy at the call site.
    if base.scheme() == "https" && url.scheme() == "http" && base.host_str() == url.host_str() && base.port_or_known_default() == Some(443) && url.port_or_known_default() == Some(80) {
        url.set_scheme("https").map_err(|()| OpdsError::InvalidUrl("could not upgrade a same-host OPDS URL to HTTPS".to_owned()))?;
        url.set_port(base.port()).map_err(|()| OpdsError::InvalidUrl("could not restore the OPDS HTTPS port".to_owned()))?;
    }
    validate_http_url(url)
}

fn validate_http_url(mut url: Url) -> Result<Url, OpdsError> {
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err(OpdsError::InvalidUrl("URL must be absolute HTTP(S), have a host, and contain no credentials".to_owned()));
    }
    url.set_fragment(None);
    Ok(url)
}

fn validated_response_url(response: &Response) -> Result<Url, OpdsError> {
    validate_http_url(response.url().clone())
}

fn normalized_media_type(value: &str) -> String {
    value.split(';').next().unwrap_or(value).trim().to_ascii_lowercase()
}

fn is_catalog_media_type(value: &str) -> bool {
    matches!(normalized_media_type(value).as_str(), OPDS_2_MEDIA_TYPE | "application/atom+xml" | "application/xml" | "text/xml")
}

fn link_target_from_media_type(value: &str) -> OpdsLinkTarget {
    if normalized_media_type(value) == "text/html" {
        OpdsLinkTarget::Web
    } else {
        OpdsLinkTarget::Catalog
    }
}

fn book_format_from_media_type(value: &str) -> Option<OpdsBookFormat> {
    match normalized_media_type(value).as_str() {
        EPUB_MEDIA_TYPE => Some(OpdsBookFormat::Epub),
        PDF_MEDIA_TYPE => Some(OpdsBookFormat::Pdf),
        MOBI_MEDIA_TYPE | "application/vnd.amazon.ebook" | "application/x-mobi8-ebook" => Some(OpdsBookFormat::Mobi),
        M4B_MEDIA_TYPE | "audio/x-m4b" | "audio/m4b" => Some(OpdsBookFormat::M4b),
        _ => None,
    }
}

fn book_format_from_url(url: &Url) -> Option<OpdsBookFormat> {
    let path = url.path().to_ascii_lowercase();
    if path.ends_with(".epub") {
        Some(OpdsBookFormat::Epub)
    } else if path.ends_with(".pdf") {
        Some(OpdsBookFormat::Pdf)
    } else if path.ends_with(".mobi") || path.ends_with(".azw") || path.ends_with(".azw3") {
        Some(OpdsBookFormat::Mobi)
    } else if path.ends_with(".m4b") {
        Some(OpdsBookFormat::M4b)
    } else {
        None
    }
}

fn is_navigation_relation(value: &str) -> bool {
    matches!(value, "subsection" | "http://opds-spec.org/sort/new" | "http://opds-spec.org/sort/popular")
}

fn is_image_relation(value: &str) -> bool {
    matches!(value, "http://opds-spec.org/image" | "http://opds-spec.org/image/thumbnail" | "cover" | "thumbnail")
}

fn safe_book_file_name(title: &str, url: &Url, book_format: OpdsBookFormat) -> String {
    let extension = format!(".{}", book_format.extension());
    let url_stem = url.path_segments().and_then(Iterator::last).and_then(|segment| urlencoding::decode(segment).ok()).map(|segment| segment.into_owned()).filter(|segment| segment.to_ascii_lowercase().ends_with(&extension));
    let candidate = url_stem.unwrap_or_else(|| format!("{}{}", nonempty(title).unwrap_or("Untitled"), extension));
    let mut sanitized = candidate.chars().map(|character| if character.is_control() || matches!(character, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { character }).collect::<String>();
    if !sanitized.to_ascii_lowercase().ends_with(&extension) {
        sanitized.push_str(&extension);
    }
    if sanitized.len() > 240 {
        sanitized.truncate(sanitized.floor_char_boundary(240 - extension.len()));
        sanitized.push_str(&extension);
    }
    if sanitized == extension {
        format!("Untitled{extension}")
    } else {
        sanitized
    }
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, HeaderMap, Response, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn ranged_test_epub() -> Vec<u8> {
        use std::io::Write as _;
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        archive.start_file("mimetype", stored).unwrap();
        archive.write_all(b"application/epub+zip").unwrap();
        archive.start_file("META-INF/container.xml", deflated).unwrap();
        archive.write_all(br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#).unwrap();
        archive.start_file("OEBPS/content.opf", deflated).unwrap();
        archive
            .write_all(
                br#"<package xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>Ranged</dc:title></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="cover" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/><item id="body" href="body.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="body"/></spine></package>"#,
            )
            .unwrap();
        archive.start_file("OEBPS/nav.xhtml", deflated).unwrap();
        archive.write_all(br#"<html><body><nav epub:type="toc"><ol><li><a href="body.xhtml">Chapter</a></li></ol></nav></body></html>"#).unwrap();
        archive.start_file("OEBPS/cover.jpg", stored).unwrap();
        archive.write_all(b"small-cover-placeholder").unwrap();
        archive.start_file("OEBPS/body.xhtml", stored).unwrap();
        archive.write_all(&vec![b'x'; 2 * 1024 * 1024]).unwrap();
        archive.finish().unwrap().into_inner()
    }

    #[tokio::test]
    async fn ranged_epub_download_materializes_only_metadata_navigation_and_cover() {
        let epub = Arc::new(ranged_test_epub());
        let response_epub = epub.clone();
        let application = Router::new().route(
            "/book.epub",
            get(move |headers: HeaderMap| {
                let epub = response_epub.clone();
                async move {
                    let range = headers.get(header::RANGE).and_then(|value| value.to_str().ok()).unwrap();
                    let (start, end) = if let Some(suffix) = range.strip_prefix("bytes=-") {
                        let suffix = suffix.parse::<usize>().unwrap().min(epub.len());
                        (epub.len() - suffix, epub.len() - 1)
                    } else {
                        let bounds = range.strip_prefix("bytes=").unwrap();
                        let (start, end) = bounds.split_once('-').unwrap();
                        (start.parse::<usize>().unwrap(), end.parse::<usize>().unwrap())
                    };
                    Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(header::CONTENT_TYPE, EPUB_MEDIA_TYPE)
                        .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{}", epub.len()))
                        .header(header::ETAG, "\"stable\"")
                        .body(Body::from(epub[start..=end].to_vec()))
                        .unwrap()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let acquisition = OpdsDirectAcquisition { url: format!("http://{address}/book.epub"), kind: OpdsAcquisitionKind::OpenAccess, book_format: OpdsBookFormat::Epub, suggested_file_name: "book.epub".to_owned(), price: None };
        let downloaded = OpdsClient::new().unwrap().download_epub_metadata_with_limit(&acquisition, temporary.path(), 8 * 1024 * 1024, true).await.unwrap();
        assert!(downloaded.is_metadata_only());
        assert!(downloaded.transferred_bytes() < epub.len() as u64 / 2);
        assert_eq!(read_epub_entry(downloaded.path(), "META-INF/container.xml", MAX_EPUB_CONTAINER_BYTES).unwrap(), br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#);
        assert!(read_epub_entry(downloaded.path(), "OEBPS/content.opf", MAX_EPUB_PACKAGE_BYTES).is_ok());
        assert!(read_epub_entry(downloaded.path(), "OEBPS/nav.xhtml", MAX_EPUB_NAVIGATION_BYTES).is_ok());
        assert_eq!(read_epub_entry(downloaded.path(), "OEBPS/cover.jpg", MAX_EPUB_COVER_BYTES).unwrap(), b"small-cover-placeholder");
        server.abort();
    }

    const OPDS_1: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <id>catalog</id><title>Test catalog</title><updated>2024-01-01T00:00:00Z</updated>
  <link rel="self" href="/opds" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  <link rel="next" href="?page=2" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  <entry><id>book-1</id><title>A Book</title><updated>2024-01-01T00:00:00Z</updated><author><name>An Author</name><uri>authors/an-author</uri></author><summary>A summary</summary><link rel="http://opds-spec.org/image/thumbnail" href="covers/1.jpg" type="image/jpeg"/><link rel="http://opds-spec.org/acquisition/open-access" href="books/1.epub" type="application/epub+zip"/></entry>
</feed>"#;

    const OPDS_2: &str = r#"{
      "metadata":{"title":"Test catalog","numberOfItems":1},
      "links":[{"rel":"self","href":"/v2","type":"application/opds+json"},{"rel":["next"],"href":"?page=2","type":"application/opds+json"},{"rel":"search","href":"search{?query,title}","type":"application/opds+json","templated":true}],
      "navigation":[{"title":"Recent","href":"recent","type":"application/opds+json","rel":"http://opds-spec.org/sort/popular"}],
      "publications":[{"metadata":{"identifier":"urn:isbn:1","title":{"en":"A Book"},"author":["One",{"name":"Two","sortAs":"Two, Author","identifier":"https://isni.org/isni/0000000121400562","links":[{"href":"authors/two","type":"application/opds+json"}]}]},"links":[{"rel":"download","href":"../book.epub","type":"application/epub+zip"}],"images":[{"href":"cover.jpg","type":"image/jpeg"}]}]
    }"#;

    #[test]
    fn built_in_endpoints_are_complete_and_safe() {
        let endpoints = built_in_opds_endpoints();
        assert_eq!(endpoints.len(), 4);
        assert_eq!(endpoints.iter().map(|endpoint| endpoint.id.as_str()).collect::<HashSet<_>>().len(), endpoints.len());
        assert!(endpoints.iter().all(|endpoint| !endpoint.title.trim().is_empty() && !endpoint.description.trim().is_empty() && !endpoint.languages.is_empty()));
        assert!(endpoints.iter().all(|endpoint| endpoint.id.as_str() != "open-library"));
        assert!(endpoints.iter().all(|endpoint| !matches!(endpoint.id.as_str(), "chitanka" | "ebooks-gratuits" | "office-of-the-historian")));
        assert!(endpoints.iter().all(|endpoint| endpoint.languages.iter().any(|language| matches!(language.as_str(), "Multilingual" | "English" | "Swedish" | "French"))));
        assert!(endpoints.iter().all(|endpoint| parse_http_url(&endpoint.url).is_ok()));
    }

    #[test]
    fn performance_log_urls_exclude_queries_and_fragments() {
        assert_eq!(profile_log_url("https://catalog.example/search?q=private#result"), "https://catalog.example/search");
    }

    #[test]
    fn validates_and_canonicalizes_catalogue_language_tags() {
        assert_eq!(canonical_language_tag("EN_us").as_deref(), Some("en-US"));
        assert_eq!(canonical_language_tag("zh-hant-tw").as_deref(), Some("zh-Hant-TW"));
        assert_eq!(canonical_language_tag("iw-IL").as_deref(), Some("he-IL"));
        assert_eq!(canonical_language_tag("i-klingon").as_deref(), Some("tlh"));
        assert_eq!(canonical_language_tag("x-catalogue").as_deref(), Some("x-catalogue"));
        assert_eq!(canonical_language_tag("en--US"), None);
        assert_eq!(canonical_language_tag("zz-invalid"), None);
        assert_eq!(canonical_language_tags(["SV_se", "sv-SE", "zh-hant-tw"]), ["sv-SE", "zh-Hant-TW"]);
    }

    #[test]
    fn preserves_delta_and_http_date_retry_after_values() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "120".parse().unwrap());
        assert_eq!(retry_after(&headers, now), Some(Duration::from_secs(120)));

        headers.insert(reqwest::header::RETRY_AFTER, httpdate::fmt_http_date(now + Duration::from_secs(90)).parse().unwrap());
        assert_eq!(retry_after(&headers, now), Some(Duration::from_secs(90)));
    }

    #[tokio::test]
    #[ignore = "requires live third-party catalog servers"]
    async fn open_built_in_catalogs_are_live_and_parseable() {
        let client = OpdsClient::new().unwrap();
        for endpoint in built_in_opds_endpoints() {
            let catalog = client.fetch_catalog(&endpoint.url).await.unwrap_or_else(|error| panic!("{} failed: {error}", endpoint.title));
            assert!(!catalog.title.trim().is_empty(), "{} returned no title", endpoint.title);
            assert!(!catalog.navigation.is_empty() || !catalog.publications.is_empty() || !catalog.groups.is_empty(), "{} returned no browseable content", endpoint.title);
        }
    }

    #[tokio::test]
    #[ignore = "requires live third-party catalog servers"]
    async fn curated_catalogs_are_live_and_parseable() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../servers/opds-cache/opds.json");
        let registry: CatalogRegistry = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let endpoints = endpoints_from_registry(registry).unwrap();
        let futures = endpoints.iter().map(|endpoint| async {
            let result = OpdsClient::new().unwrap().fetch_catalog(&endpoint.url).await;
            (endpoint.title.clone(), result)
        });
        let results = futures_util::future::join_all(futures).await;
        let failures = results.into_iter().filter_map(|(title, result)| result.err().map(|error| format!("{title}: {error}"))).collect::<Vec<_>>();
        assert!(failures.is_empty(), "curated OPDS failures:\n{}", failures.join("\n"));
    }

    #[tokio::test]
    #[ignore = "requires the live Open Library OPDS service"]
    async fn open_library_browse_and_acquisitions_are_live_and_parseable() {
        let client = OpdsClient::new().unwrap();
        let root = client.fetch_catalog("https://openlibrary.org/opds/").await.unwrap();
        assert_eq!(root.version, OpdsVersion::V2_0);
        assert_eq!(root.title, "Open Library");
        assert!(!root.navigation.is_empty());
        assert!(root.groups.iter().flat_map(|group| &group.publications).any(|publication| publication.id.starts_with("https://openlibrary.org/opds/books/") && !publication.acquisitions.is_empty()));
        let navigation = client.fetch_catalog(&root.navigation[0].url).await.unwrap();
        assert!(!navigation.publications.is_empty() || !navigation.groups.is_empty());
    }

    #[test]
    fn parses_opds_1_and_resolves_relative_links() {
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), Some("application/atom+xml; profile=opds-catalog"), OPDS_1.as_bytes()).unwrap();
        assert_eq!(catalog.version, OpdsVersion::V1_2);
        assert_eq!(catalog.self_url.as_deref(), Some("https://example.com/opds"));
        assert_eq!(catalog.pagination.next.as_deref(), Some("https://example.com/catalog/root.xml?page=2"));
        assert_eq!(catalog.publications[0].authors.iter().map(|author| author.name.as_str()).collect::<Vec<_>>(), vec!["An Author"]);
        assert_eq!(catalog.publications[0].authors[0].identifiers, vec!["https://example.com/catalog/authors/an-author"]);
        assert_eq!(catalog.publications[0].authors[0].provider_link, Some(OpdsAuthorLink { url: "https://example.com/catalog/authors/an-author".to_owned(), target: OpdsLinkTarget::Web }));
        assert_eq!(catalog.publications[0].image_url.as_deref(), Some("https://example.com/catalog/covers/1.jpg"));
        let OpdsAcquisition::Direct(acquisition) = &catalog.publications[0].acquisitions[0] else { panic!("expected direct EPUB") };
        assert_eq!(acquisition.url, "https://example.com/catalog/books/1.epub");
        assert_eq!(acquisition.book_format, OpdsBookFormat::Epub);
        assert_eq!(catalog.publications[0].preferred_download.as_ref(), Some(acquisition));
    }

    #[test]
    fn retains_rich_opds_1_publication_metadata() {
        let feed = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:dc="http://purl.org/dc/terms/" xmlns:opf="http://www.idpf.org/2007/opf" xmlns:calibre="http://calibre.kovidgoyal.net/2009/metadata" xmlns:schema="https://schema.org/">
  <id>catalog</id><title>Rich catalog</title><updated>2026-08-21T00:00:00Z</updated>
  <entry><id>urn:isbn:9780306406157</id><title>Rich Book</title><published>2020-02-03T00:00:00Z</published><updated>2026-08-20T12:00:00Z</updated>
    <dc:publisher>Example Press</dc:publisher><dc:identifier>urn:isbn:9780306406157</dc:identifier>
    <dc:subject opf:authority="DDC" opf:term="510">Mathematics</dc:subject>
    <category scheme="BISAC" term="HIS036060" label="History / United States / 20th Century"/>
    <calibre:subtitle>A Documentary History</calibre:subtitle><calibre:series series_index="2" sortAs="Archive Studies, The" identifier="urn:series:archive">Archive Studies</calibre:series>
    <dc:audience>Young adult</dc:audience><schema:typicalAgeRange>12-17</schema:typicalAgeRange>
    <schema:aggregateRating ratingValue="4.25" ratingCount="12" bestRating="5" worstRating="1"/>
    <schema:interactionStatistic interactionType="https://schema.org/ReadAction" userInteractionCount="37" interactionService="Example"/>
    <schema:isFamilyFriendly>true</schema:isFamilyFriendly>
    <contributor><name>General Editor</name></contributor><dc:contributor opf:role="trl">A Translator</dc:contributor>
    <link rel="http://opds-spec.org/image" href="cover.jpg" type="image/jpeg"/>
    <link rel="http://opds-spec.org/image/thumbnail" href="thumb.jpg" type="image/jpeg"/>
    <link rel="http://opds-spec.org/acquisition/open-access" href="book.epub" type="application/epub+zip"/>
  </entry>
</feed>"#;
        let catalog = parse_catalog(Url::parse("https://example.com/opds").unwrap(), Some("application/atom+xml"), feed.as_bytes()).unwrap();
        let metadata = &catalog.publications[0].metadata;
        assert_eq!(metadata.published.as_deref(), Some("2020-02-03T00:00:00+00:00"));
        assert_eq!(metadata.modified.as_deref(), Some("2026-08-20T12:00:00+00:00"));
        assert_eq!(metadata.publishers.iter().map(|publisher| publisher.name.as_str()).collect::<Vec<_>>(), ["Example Press"]);
        assert!(metadata.identifiers.iter().any(|identifier| identifier.value == "urn:isbn:9780306406157" && identifier.scheme.as_deref() == Some("isbn") && identifier.scope == OpdsIdentifierScope::Publication));
        assert!(metadata.identifiers.iter().any(|identifier| identifier.value == "urn:isbn:9780306406157" && identifier.scope == OpdsIdentifierScope::CatalogEntry));
        assert!(metadata.subjects.iter().any(|subject| subject.authority.as_deref() == Some("DDC") && subject.code.as_deref() == Some("510") && subject.name == "Mathematics"));
        assert!(metadata.subjects.iter().any(|subject| subject.authority.as_deref() == Some("BISAC") && subject.code.as_deref() == Some("HIS036060")));
        assert_eq!(metadata.subtitle.as_deref(), Some("A Documentary History"));
        assert_eq!(
            metadata.collections[0],
            OpdsPublicationCollection {
                name: "Archive Studies".to_owned(),
                name_localizations: Vec::new(),
                sort_name: Some("Archive Studies, The".to_owned()),
                sort_name_localizations: Vec::new(),
                identifiers: vec!["urn:series:archive".to_owned()],
                kind: Some(CollectionKind::Series),
                position: Some("2".to_owned()),
                source: "atom:series".to_owned(),
            }
        );
        assert!(metadata.contributors.iter().any(|contributor| contributor.name == "General Editor" && contributor.roles == ["contributor"]));
        assert!(metadata.contributors.iter().any(|contributor| contributor.name == "A Translator" && contributor.roles == ["trl"]));
        assert_eq!(metadata.audiences[0].value, "Young adult");
        assert_eq!(metadata.age_ranges[0].value, "12-17");
        assert_eq!(metadata.ratings[0].value, "4.25");
        assert_eq!(metadata.ratings[0].rating_count, Some(12));
        assert_eq!(metadata.interactions[0].count, 37);
        assert!(metadata.properties.iter().any(|property| property.property.ends_with("isFamilyFriendly") && property.value.contains("true")));
        assert_eq!(metadata.images.iter().map(|image| image.url.as_str()).collect::<Vec<_>>(), ["https://example.com/cover.jpg", "https://example.com/thumb.jpg"]);
        assert_eq!(catalog.publications[0].image_url.as_deref(), Some("https://example.com/cover.jpg"));
    }

    #[test]
    fn preserves_non_http_opds_1_person_identifiers_without_exposing_them_as_links() {
        let feed = OPDS_1.replace("<uri>authors/an-author</uri>", "<uri>urn:isni:0000000121400562</uri>");
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), Some("application/atom+xml"), feed.as_bytes()).unwrap();
        let author = &catalog.publications[0].authors[0];
        assert_eq!(author.identifiers, vec!["urn:isni:0000000121400562"]);
        assert_eq!(author.provider_link, None);
    }

    #[test]
    fn accepts_and_preserves_html_named_entities_in_legacy_atom_xhtml() {
        let feed = OPDS_1.replace("<summary>A summary</summary>", r#"<content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>&alpha; &copy; &amp; XML</p></div></content>"#);
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), Some("application/atom+xml"), feed.as_bytes()).unwrap();
        assert_eq!(catalog.publications.len(), 1);
        let description = catalog.publications[0].description.as_ref().unwrap();
        assert!(matches!(description, OpdsDescription::Xhtml { .. }));
        let markup = description.markup().unwrap();
        assert!(markup.contains("<p>"));
        assert!(markup.contains("&alpha; &copy; &amp; XML"));
    }

    #[test]
    fn normalizes_gutenberg_work_identity_without_rewriting_rich_descriptions() {
        let feed = OPDS_1
            .replace("<id>book-1</id>", "<id>urn:gutenberg:2465:3</id>")
            .replace("<summary>A summary</summary>", r#"<content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>This edition has images.</p><p>Title: Carmen</p><p>Note: Wikipedia page about this book: <a href="https://en.wikipedia.org/wiki/Carmen_(novella)">Carmen</a></p><p>Credits: A useful credit</p><p>Summary: A useful summary</p></div></content>"#);
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), Some("application/atom+xml"), feed.as_bytes()).unwrap();
        assert_eq!(catalog.publications[0].work_id.as_deref(), Some("urn:gutenberg:2465"));
        let description = catalog.publications[0].description.as_ref().unwrap();
        assert!(matches!(description, OpdsDescription::Xhtml { .. }));
        assert!(description.markup().is_some_and(|markup| markup.contains("A useful summary")));
        assert!(description.markup().is_some_and(|markup| markup.contains("Credits:")));
        assert!(description.markup().is_some_and(|markup| markup.contains("Wikipedia")));
    }

    #[test]
    fn preserves_rich_description_markup() {
        let source =
            r#"<div class="layout"><p onclick="attack()"><strong>Safe formatting</strong></p><a href="javascript:attack()">unsafe link</a><img src="https://tracker.example/pixel" onerror="attack()"><script>attack()</script></div>"#;
        let description = typed_description(source, DescriptionFormat::Html).unwrap();
        assert_eq!(description.markup(), Some(source));
        let serialized = serde_json::to_string(&description).unwrap();
        assert!(serialized.contains("markup"));
        assert!(serialized.contains("onclick"));
    }

    #[test]
    fn preserves_opds_1_facet_groups_counts_and_active_state() {
        let feed = OPDS_1.replace(
            r#"  <link rel="next" href="?page=2" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>"#,
            r#"  <link rel="next" href="?page=2" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  <link rel="http://opds-spec.org/facet" href="?language=en" title="English" opds:facetGroup="Language" opds:activeFacet="true" thr:count="42" xmlns:opds="http://opds-spec.org/2010/catalog" xmlns:thr="http://purl.org/syndication/thread/1.0"/>
  <link rel="http://opds-spec.org/facet" href="?language=sv" title="Swedish" opds:facetGroup="Language" thr:count="7" xmlns:opds="http://opds-spec.org/2010/catalog" xmlns:thr="http://purl.org/syndication/thread/1.0"/>"#,
        );
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), None, feed.as_bytes()).unwrap();
        assert_eq!(catalog.facets.len(), 1);
        assert_eq!(catalog.facets[0].title, "Language");
        assert_eq!(catalog.facets[0].links.len(), 2);
        assert_eq!(catalog.facets[0].links[0].number_of_items, Some(42));
        assert!(catalog.facets[0].links[0].active);
        assert_eq!(catalog.facets[0].links[1].number_of_items, Some(7));
        assert!(!catalog.facets[0].links[1].active);
    }

    #[test]
    fn parses_opds_2_groups_navigation_and_pagination() {
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/v2/").unwrap(), Some(OPDS_2_MEDIA_TYPE), OPDS_2.as_bytes()).unwrap();
        assert_eq!(catalog.version, OpdsVersion::V2_0);
        assert_eq!(catalog.pagination.number_of_items, Some(1));
        assert_eq!(catalog.navigation[0].url, "https://example.com/catalog/v2/recent");
        assert_eq!(catalog.navigation[0].relations, ["http://opds-spec.org/sort/popular"]);
        assert_eq!(catalog.publications[0].authors.iter().map(|author| author.name.as_str()).collect::<Vec<_>>(), vec!["One", "Two"]);
        assert_eq!(catalog.publications[0].authors[1].sort_name.as_deref(), Some("Two, Author"));
        assert_eq!(catalog.publications[0].authors[1].identifiers, vec!["https://isni.org/isni/0000000121400562"]);
        assert_eq!(catalog.publications[0].authors[1].provider_link, Some(OpdsAuthorLink { url: "https://example.com/catalog/v2/authors/two".to_owned(), target: OpdsLinkTarget::Catalog }));
        let OpdsAcquisition::Direct(acquisition) = &catalog.publications[0].acquisitions[0] else { panic!("expected direct EPUB") };
        assert_eq!(acquisition.url, "https://example.com/catalog/book.epub");
        assert_eq!(catalog.publications[0].preferred_download.as_ref(), Some(acquisition));
    }

    #[test]
    fn retains_rich_opds_2_publication_metadata() {
        let feed = r#"{
          "metadata":{"title":"Rich catalog"},
          "publications":[{
            "metadata":{
              "identifier":["urn:isbn:9780306406157","urn:uuid:book"],
              "title":{"en":"Rich Book","sv":"Rik bok"},"sortAs":{"en":"Rich Book","sv":"Bok, Rik"},"subtitle":{"en":"A Documentary History","sv":"En dokumentär historia"},
              "author":{"name":{"en":"An Author","sv":"En författare"},"sortAs":{"en":"Author, An","sv":"Författare, En"}},
              "published":"2020-02-03","modified":"2026-08-20T12:00:00Z","audience":["Young adult","Teen"],"typicalAgeRange":"12-17",
              "publisher":{"name":{"en":"Example Press","sv":"Exempelförlaget"},"sortAs":{"en":"Example Press","sv":"Exempelförlaget"},"identifier":"https://example.com/publishers/1"},
              "aggregateRating":{"@type":"AggregateRating","ratingValue":4.5,"ratingCount":22,"reviewCount":"7","bestRating":5,"worstRating":1},
              "interactionStatistic":[{"@type":"InteractionCounter","interactionType":"https://schema.org/ReadAction","userInteractionCount":91,"interactionService":{"name":"Example Reader"}}],
              "accessibility":{"feature":["alternativeText","tableOfContents"]},
              "subject":[{"name":{"en":"Mathematics","sv":"Matematik"},"sortAs":{"en":"Mathematics","sv":"Matematik"},"scheme":"DDC","code":"510"},"History / United States / 20th Century"],
              "belongsTo":{"series":{"name":{"en":"Archive Studies","sv":"Arkivstudier"},"sortAs":{"en":"Archive Studies, The","sv":"Arkivstudier"},"identifier":"urn:series:archive","altIdentifier":[{"@id":"legacy-series-1","scheme":"example"},"urn:series:archive"],"position":2},"collection":"Documents"},
              "translator":{"name":"A Translator","identifier":"urn:isni:translator"},
              "contributor":{"name":"An Editor","role":["edt","researcher"]}
            },
            "images":[{"href":"cover.jpg","type":"image/jpeg","rel":"cover","width":1200,"height":1800},{"href":"thumb.jpg","type":"image/jpeg","rel":"thumbnail","width":200,"height":300}],
            "links":[{"rel":"download","href":"book.epub","type":"application/epub+zip"}]
          }]
        }"#;
        let catalog = parse_catalog(Url::parse("https://example.com/opds/").unwrap(), Some(OPDS_2_MEDIA_TYPE), feed.as_bytes()).unwrap();
        let metadata = &catalog.publications[0].metadata;
        assert_eq!(metadata.published.as_deref(), Some("2020-02-03"));
        assert_eq!(metadata.modified.as_deref(), Some("2026-08-20T12:00:00Z"));
        assert!(metadata.identifiers.iter().any(|identifier| identifier.value == "urn:isbn:9780306406157" && identifier.scheme.as_deref() == Some("isbn")));
        assert!(metadata.identifiers.iter().any(|identifier| identifier.value == "urn:uuid:book" && identifier.scheme.as_deref() == Some("uuid")));
        assert!(metadata.identifiers.iter().any(|identifier| identifier.scope == OpdsIdentifierScope::CatalogEntry));
        assert_eq!(metadata.publishers[0].name, "Example Press");
        assert_eq!(metadata.publishers[0].name_localizations, [OpdsLocalizedText { language: "en".to_owned(), value: "Example Press".to_owned() }, OpdsLocalizedText { language: "sv".to_owned(), value: "Exempelförlaget".to_owned() }]);
        assert_eq!(metadata.publishers[0].identifiers, ["https://example.com/publishers/1"]);
        assert_eq!(metadata.title_localizations, [OpdsLocalizedText { language: "en".to_owned(), value: "Rich Book".to_owned() }, OpdsLocalizedText { language: "sv".to_owned(), value: "Rik bok".to_owned() }]);
        assert_eq!(metadata.sort_name.as_deref(), Some("Rich Book"));
        assert_eq!(metadata.sort_name_localizations.len(), 2);
        assert_eq!(metadata.subtitle.as_deref(), Some("A Documentary History"));
        assert_eq!(metadata.subtitle_localizations.len(), 2);
        assert_eq!(catalog.publications[0].authors[0].name, "An Author");
        assert_eq!(catalog.publications[0].authors[0].name_localizations.len(), 2);
        assert_eq!(catalog.publications[0].authors[0].sort_name_localizations.len(), 2);
        assert!(metadata.subjects.iter().any(|subject| subject.authority.as_deref() == Some("DDC") && subject.code.as_deref() == Some("510")));
        assert!(metadata.subjects.iter().any(|subject| subject.name == "Mathematics" && subject.name_localizations.len() == 2 && subject.sort_name_localizations.len() == 2));
        assert!(metadata.collections.iter().any(|collection| {
            collection.name == "Archive Studies"
                && collection.sort_name.as_deref() == Some("Archive Studies, The")
                && collection.name_localizations.len() == 2
                && collection.sort_name_localizations.len() == 2
                && collection.identifiers == ["urn:series:archive", "legacy-series-1"]
                && collection.position.as_deref() == Some("2")
        }));
        assert!(metadata.collections.iter().any(|collection| collection.name == "Documents" && collection.collection_type() == Some("collection")));
        assert!(metadata.contributors.iter().any(|contributor| contributor.name == "A Translator" && contributor.roles == ["translator"]));
        assert!(metadata.contributors.iter().any(|contributor| contributor.name == "An Editor" && contributor.roles == ["edt", "researcher"]));
        assert_eq!(metadata.audiences.iter().map(|audience| audience.value.as_str()).collect::<Vec<_>>(), ["Young adult", "Teen"]);
        assert_eq!(metadata.age_ranges[0].value, "12-17");
        assert_eq!(
            metadata.ratings[0],
            OpdsAggregateRating {
                value: "4.5".to_owned(),
                rating_count: Some(22),
                review_count: Some(7),
                best_rating: Some("5".to_owned()),
                worst_rating: Some("1".to_owned()),
                source: "opds2:https://example.com/opds/#metadata.aggregateRating".to_owned()
            }
        );
        assert_eq!(metadata.interactions[0].interaction_type, "https://schema.org/ReadAction");
        assert_eq!(metadata.interactions[0].count, 91);
        assert_eq!(metadata.interactions[0].service.as_deref(), Some("Example Reader"));
        assert!(metadata.properties.iter().any(|property| property.property == "accessibility" && property.value.contains("alternativeText")));
        assert_eq!(metadata.images.len(), 2);
        assert_eq!((metadata.images[0].width, metadata.images[0].height), (Some(1200), Some(1800)));
        assert_eq!(catalog.publications[0].image_url.as_deref(), Some("https://example.com/opds/cover.jpg"));
    }

    #[test]
    fn opds_2_publication_self_link_is_the_identity_when_metadata_omits_it() {
        let feed = r#"{"metadata":{"title":"Open Library"},"publications":[{"metadata":{"title":"Watercolor"},"links":[{"rel":"self","href":"/opds/books/OL8357203M","type":"application/opds-publication+json"},{"rel":"alternate","href":"/books/OL8357203M","type":"text/html"}]}]}"#;
        let catalog = parse_catalog(Url::parse("https://openlibrary.org/opds/").unwrap(), Some(OPDS_2_MEDIA_TYPE), feed.as_bytes()).unwrap();
        assert_eq!(catalog.publications[0].id, "https://openlibrary.org/opds/books/OL8357203M");
        let OpdsAcquisition::Unsupported(action) = &catalog.publications[0].acquisitions[0] else { panic!("expected external publication action") };
        assert_eq!(action.url, "https://openlibrary.org/books/OL8357203M");
        assert_eq!(action.kind, OpdsAcquisitionKind::Generic);
    }

    #[test]
    fn malformed_opds_2_identifiers_fall_back_to_stable_publication_evidence() {
        let acquisition_feed =
            r#"{"metadata":{"title":"Catalog"},"publications":[{"metadata":{"identifier":"","title":"Book"},"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/book.epub","type":"application/epub+zip"}]}]}"#;
        let acquisition_catalog = parse_catalog(Url::parse("https://example.com/opds/").unwrap(), Some(OPDS_2_MEDIA_TYPE), acquisition_feed.as_bytes()).unwrap();
        assert_eq!(acquisition_catalog.publications[0].id, "https://example.com/book.epub");
        acquisition_catalog.publications[0].identity().validate().unwrap();

        let synthetic_feed = r#"{"metadata":{"title":"Catalog"},"publications":[{"metadata":{"identifier":"\u0001","title":"Unidentified","author":[{"name":"Author"}]}}]}"#;
        let first = parse_catalog(Url::parse("https://example.com/opds/").unwrap(), Some(OPDS_2_MEDIA_TYPE), synthetic_feed.as_bytes()).unwrap();
        let second = parse_catalog(Url::parse("https://example.com/search?query=other").unwrap(), Some(OPDS_2_MEDIA_TYPE), synthetic_feed.as_bytes()).unwrap();
        assert!(first.publications[0].id.starts_with("urn:bokheim:opds:synthetic:"));
        assert_eq!(first.publications[0].id, second.publications[0].id);
        first.publications[0].identity().validate().unwrap();
    }

    #[test]
    fn blank_opds_1_entry_id_falls_back_to_the_acquisition_url() {
        let feed = r#"<?xml version="1.0" encoding="utf-8"?><feed xmlns="http://www.w3.org/2005/Atom"><id>catalog</id><title>Catalog</title><updated>2026-01-01T00:00:00Z</updated><entry><id></id><title>Book</title><updated>2026-01-01T00:00:00Z</updated><link rel="http://opds-spec.org/acquisition/open-access" href="/book.epub" type="application/epub+zip"/></entry></feed>"#;
        let catalog = parse_catalog(Url::parse("https://example.com/opds/").unwrap(), Some("application/atom+xml"), feed.as_bytes()).unwrap();
        assert_eq!(catalog.publications[0].id, "https://example.com/book.epub");
        catalog.publications[0].identity().validate().unwrap();
    }

    #[test]
    fn preserves_opds_2_group_and_facet_browsing_metadata() {
        let feed = r#"{
          "metadata":{"title":"Browse"},
          "navigation":[{"title":"Website","href":"/about","type":"text/html"}],
          "groups":[{"metadata":{"title":"Featured","numberOfItems":12},"links":[{"rel":"self","href":"/featured","type":"application/opds+json"}],"navigation":[{"title":"More","href":"/more","type":"application/opds+json"}]}],
          "facets":[{"metadata":{"title":"Language"},"links":[{"rel":["self","http://opds-spec.org/facet"],"title":"English","href":"?language=en","type":"application/opds+json","properties":{"numberOfItems":9}}]}]
        }"#;
        let catalog = parse_catalog(Url::parse("https://example.com/opds/").unwrap(), Some(OPDS_2_MEDIA_TYPE), feed.as_bytes()).unwrap();
        assert_eq!(catalog.navigation[0].target, OpdsLinkTarget::Web);
        assert_eq!(catalog.groups[0].title, "Featured");
        assert_eq!(catalog.groups[0].self_url.as_deref(), Some("https://example.com/featured"));
        assert_eq!(catalog.groups[0].number_of_items, Some(12));
        assert_eq!(catalog.facets[0].links[0].number_of_items, Some(9));
        assert!(catalog.facets[0].links[0].active);
    }

    #[test]
    fn parses_indirect_acquisitions_prices_and_pdf_formats() {
        let opds_1 = OPDS_1.replace(
            r#"<link rel="http://opds-spec.org/acquisition/open-access" href="books/1.epub" type="application/epub+zip"/>"#,
            r#"<link rel="http://opds-spec.org/acquisition/buy" href="buy/1" type="text/html"><opds:price xmlns:opds="http://opds-spec.org/2010/catalog" currencycode="EUR">3.50</opds:price><opds:indirectAcquisition xmlns:opds="http://opds-spec.org/2010/catalog" type="application/pdf"/></link>"#,
        );
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/").unwrap(), None, opds_1.as_bytes()).unwrap();
        let OpdsAcquisition::Indirect(acquisition) = &catalog.publications[0].acquisitions[0] else { panic!("expected indirect acquisition") };
        assert_eq!(acquisition.available_formats, vec![OpdsBookFormat::Pdf]);
        assert_eq!(acquisition.price, Some(OpdsPrice { currency: "EUR".to_owned(), value: "3.50".to_owned() }));
        assert!(catalog.publications[0].preferred_download.is_none());

        let opds_2 = r#"{"metadata":{"title":"Shop"},"publications":[{"metadata":{"title":"Paper"},"links":[{"rel":"buy","href":"buy","type":"text/html","properties":{"price":{"currency":"USD","value":4.99},"indirectAcquisition":[{"type":"application/epub+zip","child":[{"type":"application/pdf"}]}]}}]}]}"#;
        let catalog = parse_catalog(Url::parse("https://example.com/opds/").unwrap(), Some(OPDS_2_MEDIA_TYPE), opds_2.as_bytes()).unwrap();
        let OpdsAcquisition::Indirect(acquisition) = &catalog.publications[0].acquisitions[0] else { panic!("expected indirect acquisition") };
        assert_eq!(acquisition.available_formats, vec![OpdsBookFormat::Epub, OpdsBookFormat::Pdf]);
        assert_eq!(acquisition.price.as_ref().map(|price| price.value.as_str()), Some("4.99"));
    }

    #[test]
    fn accepts_standalone_entries_legacy_atom_namespace_and_missing_format_mime() {
        let entry = r#"<entry xmlns="https://www.w3.org/2005/Atom"><id>one</id><title>Legacy</title><updated>2024-01-01T00:00:00Z</updated><link rel="http://opds-spec.org/acquisition/open-access" href="book.pdf"/></entry>"#;
        let catalog = parse_catalog(Url::parse("https://example.com/entry").unwrap(), Some("application/atom+xml"), entry.as_bytes()).unwrap();
        assert_eq!(catalog.publications.len(), 1);
        let acquisition = catalog.publications[0].preferred_download.as_ref().unwrap();
        assert_eq!(acquisition.book_format, OpdsBookFormat::Pdf);
        assert_eq!(acquisition.suggested_file_name, "book.pdf");
    }

    #[test]
    fn registry_validation_and_atomic_cache_reject_invalid_dynamic_data() {
        let source = CatalogSource { id: "example".to_owned(), title: "Example".to_owned(), description: "Books".to_owned(), languages: vec!["English".to_owned()], url: "https://example.com/opds".to_owned() };
        let registry = CatalogRegistry { schema_version: CATALOG_REGISTRY_SCHEMA_VERSION, revision: 3, catalogs: vec![source.clone()] };
        let endpoints = endpoints_from_registry(registry.clone()).unwrap();
        assert_eq!(endpoints[0].id.as_str(), "example");
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("catalogs.json");
        save_registry_cache(&path, &registry).unwrap();
        assert_eq!(load_registry_cache(&path).unwrap().unwrap().0, registry);
        let duplicate = CatalogRegistry { schema_version: CATALOG_REGISTRY_SCHEMA_VERSION, revision: 4, catalogs: vec![source.clone(), source] };
        assert!(endpoints_from_registry(duplicate).is_err());
        let local = CatalogRegistry {
            schema_version: CATALOG_REGISTRY_SCHEMA_VERSION,
            revision: 5,
            catalogs: vec![CatalogSource { id: "local".to_owned(), title: "Local".to_owned(), description: "Local catalog".to_owned(), languages: vec!["English".to_owned()], url: "http://127.0.0.1:8080/opds".to_owned() }],
        };
        assert!(endpoints_from_registry(local).is_err());
    }

    #[tokio::test]
    async fn discovers_opds_from_html_before_parsing_the_catalog() {
        let application = Router::new()
            .route(
                "/",
                get(|| async { Response::builder().header(header::CONTENT_TYPE, "text/html").body(Body::from(r#"<!doctype html><html><head><link rel="alternate" type="application/opds+json" href="/catalog"></head></html>"#)).unwrap() }),
            )
            .route(
                "/catalog",
                get(|| async {
                    Response::builder().header(header::CONTENT_TYPE, OPDS_2_MEDIA_TYPE).body(Body::from(r#"{"metadata":{"title":"Discovered"},"navigation":[{"title":"Books","href":"/books","type":"application/opds+json"}]}"#)).unwrap()
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
        let (catalog, profile) = OpdsClient::new().unwrap().profile_catalog_fetch(&format!("http://{address}/")).await;
        let catalog = catalog.unwrap();
        assert_eq!(catalog.title, "Discovered");
        assert_eq!(profile.http_fetches, 2);
        assert!(profile.response_bytes > 0);
        assert_eq!(profile.final_url.as_deref(), Some(format!("http://{address}/catalog").as_str()));
        assert!(profile.total >= profile.response_headers + profile.body_download + profile.discovery + profile.parsing);
        server.abort();
    }

    #[tokio::test]
    async fn conditional_fetch_returns_validators_and_accepts_not_modified() {
        let requests = Arc::new(AtomicUsize::new(0));
        let request_counter = requests.clone();
        let application = Router::new().route(
            "/catalog",
            get(move |headers: HeaderMap| {
                request_counter.fetch_add(1, Ordering::SeqCst);
                async move {
                    if headers.get(header::IF_NONE_MATCH).and_then(|value| value.to_str().ok()) == Some("\"catalog-v1\"") {
                        return Response::builder().status(StatusCode::NOT_MODIFIED).body(Body::empty()).unwrap();
                    }
                    Response::builder()
                        .header(header::ETAG, "\"catalog-v1\"")
                        .header(header::LAST_MODIFIED, "Wed, 21 Oct 2015 07:28:00 GMT")
                        .header(header::CONTENT_TYPE, OPDS_2_MEDIA_TYPE)
                        .body(Body::from(r#"{"metadata":{"title":"Current"},"navigation":[{"title":"Books","href":"/books","type":"application/opds+json"}]}"#))
                        .unwrap()
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
        let client = OpdsClient::new().unwrap();
        let url = format!("http://{address}/catalog");
        let OpdsConditionalCatalog::Modified { catalog, validators } = client.fetch_catalog_conditional(&url, &OpdsHttpValidators::default()).await.unwrap() else {
            panic!("initial request must return the catalog");
        };
        assert_eq!(catalog.title, "Current");
        assert_eq!(validators.etag.as_deref(), Some("\"catalog-v1\""));
        assert_eq!(validators.last_modified.as_deref(), Some("Wed, 21 Oct 2015 07:28:00 GMT"));
        assert_eq!(client.fetch_catalog_conditional(&url, &validators).await.unwrap(), OpdsConditionalCatalog::NotModified);
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[test]
    fn paid_borrowed_and_preview_epubs_are_not_offered_as_free_downloads() {
        for relation in ["http://opds-spec.org/acquisition/buy", "http://opds-spec.org/acquisition/borrow", "http://opds-spec.org/acquisition/sample"] {
            let feed = OPDS_1.replace("http://opds-spec.org/acquisition/open-access", relation);
            let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), None, feed.as_bytes()).unwrap();
            assert!(catalog.publications[0].preferred_download.is_none(), "{relation} was classified as a direct download");
        }
        let priced_generic =
            r#"{"metadata":{"title":"Shop"},"publications":[{"metadata":{"title":"Priced"},"links":[{"rel":"acquisition","href":"book.epub","type":"application/epub+zip","properties":{"price":{"currency":"EUR","value":"1.00"}}}]}]}"#;
        let catalog = parse_catalog(Url::parse("https://example.com/opds/").unwrap(), Some(OPDS_2_MEDIA_TYPE), priced_generic.as_bytes()).unwrap();
        assert!(catalog.publications[0].preferred_download.is_none());
    }

    #[test]
    fn rejects_non_http_urls_and_embedded_credentials() {
        assert!(parse_http_url("file:///tmp/catalog.xml").is_err());
        assert!(parse_http_url("https://user:secret@example.com/opds").is_err());
        assert!(resolve_url(&Url::parse("https://example.com/opds").unwrap(), "javascript:alert(1)").is_err());
    }

    #[test]
    fn upgrades_same_host_http_links_from_an_https_catalog() {
        let base = Url::parse("https://opds.openedition.org/").unwrap();
        assert_eq!(resolve_url(&base, "http://opds.openedition.org/?platform=OB&core=publishers").unwrap().as_str(), "https://opds.openedition.org/?platform=OB&core=publishers");
        assert_eq!(resolve_url(&base, "http://other.example/catalog").unwrap().as_str(), "http://other.example/catalog");
        assert_eq!(resolve_url(&base, "http://opds.openedition.org:8080/catalog").unwrap().as_str(), "http://opds.openedition.org:8080/catalog");
    }

    #[test]
    fn accepts_openedition_year_only_published_dates() {
        let feed = OPDS_1.replace("<summary>A summary</summary>", "<summary>A summary</summary><published>2013</published>");
        let catalog = parse_catalog(Url::parse("https://opds.openedition.org/books").unwrap(), Some("application/atom+xml"), feed.as_bytes()).unwrap();
        assert_eq!(catalog.publications[0].title, "A Book");

        let malformed = OPDS_1.replace("<summary>A summary</summary>", "<summary>A summary</summary><published>not-a-date</published>");
        assert!(parse_catalog(Url::parse("https://example.com/books").unwrap(), Some("application/atom+xml"), malformed.as_bytes()).is_err());
    }

    #[test]
    fn sanitizes_file_names() {
        assert_eq!(safe_book_file_name("Bad: / title", &Url::parse("https://example.com/download?id=1").unwrap(), OpdsBookFormat::Epub), "Bad_ _ title.epub");
    }

    #[test]
    fn opds_1_honors_feed_xml_base() {
        let feed = OPDS_1.replace("<feed xmlns=\"http://www.w3.org/2005/Atom\">", "<feed xmlns=\"http://www.w3.org/2005/Atom\" xml:base=\"https://cdn.example/books/\">");
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), None, feed.as_bytes()).unwrap();
        let OpdsAcquisition::Direct(acquisition) = &catalog.publications[0].acquisitions[0] else { panic!("expected direct EPUB") };
        assert_eq!(acquisition.url, "https://cdn.example/books/books/1.epub");
    }
}
