use atom_syndication::{Entry as AtomEntry, Feed, Link as AtomLink, TextType};
use futures_util::StreamExt;
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};
use reqwest::{Client, RequestBuilder, Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use uri_template::{HashValues, UriTemplate};

mod litteraturbanken;
mod registry;

pub use registry::{CatalogRegistry, CatalogSource, CATALOG_REGISTRY_SCHEMA_VERSION};

const MAX_CATALOG_BYTES: u64 = 24 * 1024 * 1024;
const MAX_REGISTRY_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SEARCH_DESCRIPTION_BYTES: u64 = 1024 * 1024;
// Bounds untrusted third-party catalog downloads before they enter a library;
// synchronized storage itself is governed by the user's account quota.
const MAX_OPDS_DOWNLOAD_BYTES: u64 = 1_000_000_000;
const NETWORK_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CUSTOM_ENDPOINTS: usize = 64;
const EPUB_MEDIA_TYPE: &str = "application/epub+zip";
const PDF_MEDIA_TYPE: &str = "application/pdf";
const MOBI_MEDIA_TYPE: &str = "application/x-mobipocket-ebook";
const FB2_MEDIA_TYPE: &str = "application/x-fictionbook+xml";
const CBZ_MEDIA_TYPE: &str = "application/vnd.comicbook+zip";
const WEBPUB_MEDIA_TYPE: &str = "application/webpub+zip";
const M4B_MEDIA_TYPE: &str = "audio/mp4";
const OPDS_2_MEDIA_TYPE: &str = "application/opds+json";
const OPENSEARCH_MEDIA_TYPE: &str = "application/opensearchdescription+xml";

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Type)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
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
        open_endpoint("open-library", "Open Library", "A multilingual Open Library and Internet Archive catalog with public-domain downloads and library lending links.", &["Multilingual"], "https://openlibrary.org/opds/"),
        open_endpoint("unglue-it", "Unglue.it", "Open and Creative Commons ebooks collected by the Free Ebook Foundation.", &["Multilingual"], "https://unglue.it/api/opds/"),
        open_endpoint("textos-info", "textos.info", "A Spanish-language catalog of freely available literature.", &["Spanish"], "https://www.textos.info/catalogo.atom"),
        open_endpoint("ebooks-gratuits", "Ebooks Gratuits", "French-language public-domain and freely distributed ebooks.", &["French"], "https://www.ebooksgratuits.com/opds/index.php"),
        open_endpoint("office-of-the-historian", "Office of the Historian", "The official Foreign Relations of the United States ebook collection.", &["English"], "https://history.state.gov/api/v1/catalog"),
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
    endpoints_from_registry_with_policy(registry, RegistryUrlPolicy::HttpsOnly)
}

pub fn validate_curated_registry(registry: &CatalogRegistry) -> Result<(), OpdsError> {
    let endpoints = endpoints_from_registry(registry.clone())?;
    if endpoints.is_empty() {
        return Err(OpdsError::InvalidEndpoint("the curated OPDS registry must contain at least one catalog".to_owned()));
    }
    Ok(())
}

pub fn custom_endpoints_from_registry(registry: CatalogRegistry) -> Result<Vec<OpdsEndpoint>, OpdsError> {
    endpoints_from_registry_with_policy(registry, RegistryUrlPolicy::HttpOrHttps)
}

fn endpoints_from_registry_with_policy(registry: CatalogRegistry, url_policy: RegistryUrlPolicy) -> Result<Vec<OpdsEndpoint>, OpdsError> {
    if registry.schema_version != CATALOG_REGISTRY_SCHEMA_VERSION {
        return Err(OpdsError::InvalidEndpoint(format!("unsupported registry schema {}", registry.schema_version)));
    }
    if registry.revision == 0 {
        return Err(OpdsError::InvalidEndpoint("registry revision must be greater than zero".to_owned()));
    }
    let mut ids = std::collections::HashSet::new();
    registry.catalogs.into_iter().map(|source| endpoint_from_source(source, &mut ids, url_policy)).collect()
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
    load_registry_cache_with_policy(path, RegistryUrlPolicy::HttpsOnly)
}

pub fn load_custom_registry_cache(path: &Path) -> Result<Option<(CatalogRegistry, Vec<OpdsEndpoint>)>, OpdsError> {
    let Some(mut value) = load_registry_cache_value(path)? else { return Ok(None) };
    if value.get("schema_version").and_then(Value::as_u64) == Some(1) {
        value["schema_version"] = Value::from(CATALOG_REGISTRY_SCHEMA_VERSION);
        if let Some(catalogs) = value.get_mut("catalogs").and_then(Value::as_array_mut) {
            for catalog in catalogs {
                if let Some(catalog) = catalog.as_object_mut() {
                    catalog.remove("access");
                    catalog.remove("authentication");
                }
            }
        }
    }
    let registry: CatalogRegistry = serde_json::from_value(value).map_err(|error| OpdsError::InvalidEndpoint(error.to_string()))?;
    let endpoints = endpoints_from_registry_with_policy(registry.clone(), RegistryUrlPolicy::HttpOrHttps)?;
    Ok(Some((registry, endpoints)))
}

fn load_registry_cache_with_policy(path: &Path, url_policy: RegistryUrlPolicy) -> Result<Option<(CatalogRegistry, Vec<OpdsEndpoint>)>, OpdsError> {
    let Some(value) = load_registry_cache_value(path)? else { return Ok(None) };
    let registry: CatalogRegistry = serde_json::from_value(value).map_err(|error| OpdsError::InvalidEndpoint(error.to_string()))?;
    let endpoints = endpoints_from_registry_with_policy(registry.clone(), url_policy)?;
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

/// Owns the persisted curated and device-local endpoint registries.
///
/// Invalid or missing caches are ignored during startup so bundled endpoints
/// remain available. Mutations are persisted before the in-memory state is
/// changed.
#[derive(Debug)]
pub struct OpdsEndpointStore {
    curated_cache_path: PathBuf,
    custom_cache_path: PathBuf,
    curated_revision: u64,
    curated: Vec<OpdsEndpoint>,
    custom: Vec<OpdsEndpoint>,
}

impl OpdsEndpointStore {
    pub fn load(curated_cache_path: PathBuf, custom_cache_path: PathBuf) -> Self {
        let cached_curated = load_registry_cache(&curated_cache_path).ok().flatten().filter(|(_, endpoints)| !endpoints.is_empty());
        let curated_revision = cached_curated.as_ref().map(|(registry, _)| registry.revision).unwrap_or(0);
        let curated = cached_curated.map(|(_, endpoints)| endpoints).unwrap_or_else(built_in_opds_endpoints);
        let custom = load_custom_registry_cache(&custom_cache_path).ok().flatten().map(|(_, endpoints)| endpoints).unwrap_or_default();
        Self { curated_cache_path, custom_cache_path, curated_revision, curated, custom }
    }

    pub fn endpoints(&self) -> Vec<OpdsEndpoint> {
        let mut endpoints = built_in_opds_endpoints();
        merge_endpoints(&mut endpoints, self.curated.iter().cloned());
        merge_endpoints(&mut endpoints, self.custom.iter().cloned());
        endpoints
    }

    pub fn custom_endpoints(&self) -> &[OpdsEndpoint] {
        &self.custom
    }

    pub fn is_curated_endpoint(&self, endpoint: &OpdsEndpoint) -> bool {
        self.curated.iter().any(|candidate| candidate.id == endpoint.id)
    }

    pub fn add_custom_endpoint(&mut self, title: &str, url: &str) -> Result<OpdsEndpoint, OpdsError> {
        let registry = CatalogRegistry {
            schema_version: CATALOG_REGISTRY_SCHEMA_VERSION,
            revision: 1,
            catalogs: vec![CatalogSource {
                id: format!("custom-{}", uuid::Uuid::new_v4().simple()),
                title: title.to_owned(),
                description: "User-supplied OPDS catalog".to_owned(),
                languages: vec!["Unspecified".to_owned()],
                url: url.to_owned(),
            }],
        };
        let endpoint = custom_endpoints_from_registry(registry)?.into_iter().next().ok_or_else(|| OpdsError::InvalidEndpoint("custom OPDS endpoint was not created".to_owned()))?;
        if self.custom.len() >= MAX_CUSTOM_ENDPOINTS {
            return Err(OpdsError::InvalidEndpoint(format!("at most {MAX_CUSTOM_ENDPOINTS} custom OPDS catalogs can be configured")));
        }
        if self.custom.iter().any(|candidate| candidate.url == endpoint.url) {
            return Err(OpdsError::InvalidEndpoint("this custom OPDS catalog is already configured".to_owned()));
        }
        let mut custom = self.custom.clone();
        custom.push(endpoint.clone());
        save_registry_cache(&self.custom_cache_path, &registry_from_endpoints(1, &custom))?;
        self.custom = custom;
        Ok(endpoint)
    }

    pub fn remove_custom_endpoint(&mut self, endpoint_id: &OpdsEndpointId) -> Result<(), OpdsError> {
        let mut custom = self.custom.clone();
        let previous_len = custom.len();
        custom.retain(|endpoint| &endpoint.id != endpoint_id);
        if custom.len() == previous_len {
            return Err(OpdsError::InvalidEndpoint("custom OPDS endpoint was not found".to_owned()));
        }
        save_registry_cache(&self.custom_cache_path, &registry_from_endpoints(1, &custom))?;
        self.custom = custom;
        Ok(())
    }

    pub fn replace_curated_registry(&mut self, registry: CatalogRegistry) -> Result<(), OpdsError> {
        let endpoints = endpoints_from_registry(registry.clone())?;
        if endpoints.is_empty() {
            return Err(OpdsError::InvalidEndpoint("the curated OPDS registry is empty".to_owned()));
        }
        if registry.revision < self.curated_revision {
            return Err(OpdsError::InvalidEndpoint(format!("refusing stale OPDS registry revision {} because revision {} is cached", registry.revision, self.curated_revision)));
        }
        if registry.revision == self.curated_revision && self.curated_revision != 0 && endpoints != self.curated {
            return Err(OpdsError::InvalidEndpoint(format!("OPDS registry revision {} changed without a revision increment", registry.revision)));
        }
        save_registry_cache(&self.curated_cache_path, &registry)?;
        self.curated = endpoints;
        self.curated_revision = registry.revision;
        Ok(())
    }
}

fn merge_endpoints(endpoints: &mut Vec<OpdsEndpoint>, replacements: impl IntoIterator<Item = OpdsEndpoint>) {
    for endpoint in replacements {
        if let Some(existing) = endpoints.iter_mut().find(|existing| existing.id == endpoint.id) {
            *existing = endpoint;
        } else {
            endpoints.push(endpoint);
        }
    }
}

#[derive(Clone, Copy)]
enum RegistryUrlPolicy {
    HttpsOnly,
    HttpOrHttps,
}

fn endpoint_from_source(source: CatalogSource, ids: &mut std::collections::HashSet<String>, url_policy: RegistryUrlPolicy) -> Result<OpdsEndpoint, OpdsError> {
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
    if matches!(url_policy, RegistryUrlPolicy::HttpsOnly) && !url.starts_with("https://") {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpdsVersion {
    V1_2,
    V2_0,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpdsCatalogFreshness {
    #[default]
    Current,
    Stale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpdsAcquisitionKind {
    OpenAccess,
    Borrow,
    Buy,
    Preview,
    Subscribe,
    Generic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpdsLinkTarget {
    Catalog,
    Web,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsCatalogLink {
    pub title: String,
    pub url: String,
    pub target: OpdsLinkTarget,
    pub number_of_items: Option<u32>,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpdsSearch {
    UriTemplate { base_url: String, template: String },
    OpenSearchDescription { url: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsSearchQuery {
    pub query: String,
    pub title: Option<String>,
    pub author: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsPagination {
    pub first: Option<String>,
    pub previous: Option<String>,
    pub next: Option<String>,
    pub last: Option<String>,
    pub number_of_items: Option<u32>,
    pub items_per_page: Option<u32>,
    pub current_page: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpdsBookFormat {
    Epub,
    Pdf,
    Mobi,
    Fb2,
    Cbz,
    WebPub,
    M4b,
}

impl OpdsBookFormat {
    fn media_type(self) -> &'static str {
        match self {
            Self::Epub => EPUB_MEDIA_TYPE,
            Self::Pdf => PDF_MEDIA_TYPE,
            Self::Mobi => MOBI_MEDIA_TYPE,
            Self::Fb2 => FB2_MEDIA_TYPE,
            Self::Cbz => CBZ_MEDIA_TYPE,
            Self::WebPub => WEBPUB_MEDIA_TYPE,
            Self::M4b => M4B_MEDIA_TYPE,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Mobi => "mobi",
            Self::Fb2 => "fb2",
            Self::Cbz => "cbz",
            Self::WebPub => "webpub",
            Self::M4b => "m4b",
        }
    }

    fn accepts_media_type(self, value: &str) -> bool {
        book_format_from_media_type(value) == Some(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsPrice {
    pub currency: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsDirectAcquisition {
    pub url: String,
    pub kind: OpdsAcquisitionKind,
    pub book_format: OpdsBookFormat,
    pub suggested_file_name: String,
    pub price: Option<OpdsPrice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsIndirectAcquisition {
    pub url: String,
    pub kind: OpdsAcquisitionKind,
    pub media_type: Option<String>,
    pub available_formats: Vec<OpdsBookFormat>,
    pub price: Option<OpdsPrice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsUnsupportedAcquisition {
    pub url: String,
    pub kind: OpdsAcquisitionKind,
    pub media_type: Option<String>,
    pub price: Option<OpdsPrice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "delivery", rename_all = "snake_case")]
pub enum OpdsAcquisition {
    Direct(OpdsDirectAcquisition),
    Indirect(OpdsIndirectAcquisition),
    Unsupported(OpdsUnsupportedAcquisition),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsAuthorCredit {
    pub name: String,
    pub sort_name: Option<String>,
    pub identifiers: Vec<String>,
    pub provider_link: Option<OpdsAuthorLink>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsAuthorLink {
    pub url: String,
    pub target: OpdsLinkTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsPublication {
    pub id: String,
    pub work_id: Option<String>,
    pub title: String,
    pub authors: Vec<OpdsAuthorCredit>,
    pub languages: Vec<String>,
    pub description: Option<OpdsDescription>,
    pub image_url: Option<String>,
    pub preferred_download: Option<OpdsDirectAcquisition>,
    pub acquisitions: Vec<OpdsAcquisition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum OpdsDescription {
    PlainText {
        text: String,
    },
    Html {
        #[serde(skip, default)]
        #[specta(skip)]
        markup: String,
        safe_html: String,
        plain_text: String,
    },
    Xhtml {
        #[serde(skip, default)]
        #[specta(skip)]
        markup: String,
        safe_html: String,
        plain_text: String,
    },
}

impl OpdsDescription {
    pub fn plain_text(&self) -> &str {
        match self {
            Self::PlainText { text } => text,
            Self::Html { plain_text, .. } | Self::Xhtml { plain_text, .. } => plain_text,
        }
    }

    pub fn markup(&self) -> Option<&str> {
        match self {
            Self::PlainText { .. } => None,
            Self::Html { markup, .. } | Self::Xhtml { markup, .. } => Some(markup),
        }
    }

    pub fn safe_html(&self) -> Option<&str> {
        match self {
            Self::PlainText { .. } => None,
            Self::Html { safe_html, .. } | Self::Xhtml { safe_html, .. } => Some(safe_html),
        }
    }
}

impl OpdsPublication {
    pub fn identity(&self) -> OpdsPublicationIdentity {
        OpdsPublicationIdentity { publication_id: self.id.clone(), work_id: self.work_id.clone() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpdsOwnershipStatus {
    NotOwned,
    OtherEditionOwned,
    ExactEditionOwned,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsPublicationOwnership {
    pub publication_id: String,
    pub status: OpdsOwnershipStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsGroup {
    pub title: String,
    pub self_url: Option<String>,
    pub number_of_items: Option<u32>,
    pub navigation: Vec<OpdsCatalogLink>,
    pub publications: Vec<OpdsPublication>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsFacet {
    pub title: String,
    pub links: Vec<OpdsCatalogLink>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
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
    pub search: Option<OpdsSearch>,
}

#[derive(Debug)]
pub enum OpdsError {
    InvalidEndpoint(String),
    InvalidUrl(String),
    Network(reqwest::Error),
    TimedOut,
    HttpStatus(reqwest::StatusCode),
    ResponseTooLarge { limit: u64 },
    UnsupportedCatalogFormat,
    InvalidCatalog(String),
    NonFreeAcquisition,
    SearchUnavailable,
    InvalidSearchTemplate(String),
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
            Self::HttpStatus(status) => write!(formatter, "OPDS server returned HTTP {status}"),
            Self::ResponseTooLarge { limit } => write!(formatter, "OPDS response exceeds the {limit}-byte limit"),
            Self::UnsupportedCatalogFormat => formatter.write_str("response is neither an OPDS 1.2 Atom feed nor an OPDS 2.0 JSON feed"),
            Self::InvalidCatalog(error) => write!(formatter, "invalid OPDS catalog: {error}"),
            Self::NonFreeAcquisition => formatter.write_str("only unpriced direct open-access or generic EPUB/PDF acquisitions can be imported"),
            Self::SearchUnavailable => formatter.write_str("this OPDS catalog does not advertise search"),
            Self::InvalidSearchTemplate(error) => write!(formatter, "invalid OPDS search template: {error}"),
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
    http: Client,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsHttpValidators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub enum OpdsConditionalCatalog {
    NotModified,
    Modified { catalog: OpdsCatalog, validators: OpdsHttpValidators },
}

impl OpdsClient {
    pub fn new() -> Result<Self, OpdsError> {
        let http = Client::builder().connect_timeout(Duration::from_secs(10)).redirect(reqwest::redirect::Policy::limited(10)).user_agent(format!("Bokheim/{} OPDS", env!("CARGO_PKG_VERSION"))).build()?;
        Ok(Self { http })
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

    pub async fn search(&self, catalog: &OpdsCatalog, query: &str) -> Result<OpdsCatalog, OpdsError> {
        let search = catalog.search.as_ref().ok_or(OpdsError::SearchUnavailable)?;
        self.search_definition(search, &OpdsSearchQuery { query: query.to_owned(), title: None, author: None }).await
    }

    pub async fn search_definition(&self, search: &OpdsSearch, query: &OpdsSearchQuery) -> Result<OpdsCatalog, OpdsError> {
        validate_search_query(query)?;
        let (base_url, template) = match search {
            OpdsSearch::UriTemplate { base_url, template } => (parse_http_url(base_url)?, template.clone()),
            OpdsSearch::OpenSearchDescription { url } => {
                let description_url = parse_http_url(url)?;
                let response = self.send(self.request(description_url.clone())).await?;
                let final_url = validated_response_url(&response)?;
                let bytes = read_bounded(response, MAX_SEARCH_DESCRIPTION_BYTES).await?;
                (final_url, parse_open_search_template(&bytes)?)
            }
        };
        let expanded = expand_search_template(&template, query)?;
        let url = resolve_url(&base_url, &expanded)?;
        self.fetch_catalog_url(url, MAX_CATALOG_BYTES).await
    }

    pub async fn download_book(&self, acquisition: &OpdsDirectAcquisition, temporary_directory: &Path) -> Result<DownloadedOpdsBook, OpdsError> {
        if !is_importable_acquisition(acquisition) {
            return Err(OpdsError::NonFreeAcquisition);
        }
        let url = parse_http_url(&acquisition.url)?;
        let request = self.request(url.clone()).header(reqwest::header::ACCEPT, acquisition.book_format.media_type());
        let response = self.send(litteraturbanken::download_request(request, &url)).await?;
        let _ = validated_response_url(&response)?;
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(normalized_media_type);
        if media_type.as_deref().is_some_and(|value| !acquisition.book_format.accepts_media_type(value) && !matches!(value, "application/octet-stream" | "application/zip" | "binary/octet-stream")) {
            return Err(OpdsError::UnsupportedBookResponse(media_type));
        }
        if response.content_length().is_some_and(|length| length > MAX_OPDS_DOWNLOAD_BYTES) {
            return Err(OpdsError::ResponseTooLarge { limit: MAX_OPDS_DOWNLOAD_BYTES });
        }
        tokio::fs::create_dir_all(temporary_directory).await?;
        let path = temporary_directory.join(format!("opds-{}.{}", uuid::Uuid::new_v4(), acquisition.book_format.extension()));
        let guard = DownloadedOpdsBook { path, file_name: acquisition.suggested_file_name.clone() };
        let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&guard.path).await?;
        let mut stream = response.bytes_stream();
        let mut downloaded = 0_u64;
        while let Some(chunk) = tokio::time::timeout(NETWORK_IDLE_TIMEOUT, stream.next()).await.map_err(|_| OpdsError::TimedOut)? {
            let chunk = chunk?;
            downloaded = downloaded.checked_add(chunk.len() as u64).ok_or(OpdsError::ResponseTooLarge { limit: MAX_OPDS_DOWNLOAD_BYTES })?;
            if downloaded > MAX_OPDS_DOWNLOAD_BYTES {
                return Err(OpdsError::ResponseTooLarge { limit: MAX_OPDS_DOWNLOAD_BYTES });
            }
            file.write_all(&chunk).await?;
        }
        if downloaded == 0 {
            return Err(OpdsError::InvalidCatalog("book acquisition returned an empty response".to_owned()));
        }
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        Ok(guard)
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
        let request = self.request(url.clone()).header(reqwest::header::ACCEPT, acquisition.book_format.media_type());
        let response = self.send(litteraturbanken::download_request(request, &url)).await?;
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
            return litteraturbanken::fetch_catalog(&self.http, url, byte_limit).await;
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
            let catalog = litteraturbanken::fetch_catalog(&self.http, url, byte_limit).await?;
            return Ok(OpdsConditionalCatalog::Modified { catalog, validators: OpdsHttpValidators::default() });
        }
        let mut url = url;
        for discovery_attempt in 0..=1 {
            let mut request = self.request(url).header(reqwest::header::ACCEPT, format!("{OPDS_2_MEDIA_TYPE}, application/atom+xml;profile=opds-catalog, application/atom+xml;q=0.9, text/html;q=0.2"));
            if discovery_attempt == 0 {
                if let Some(etag) = validators.etag.as_deref() {
                    request = request.header(reqwest::header::IF_NONE_MATCH, etag);
                }
                if let Some(last_modified) = validators.last_modified.as_deref() {
                    request = request.header(reqwest::header::IF_MODIFIED_SINCE, last_modified);
                }
            }
            let response = tokio::time::timeout(NETWORK_IDLE_TIMEOUT, request.send()).await.map_err(|_| OpdsError::TimedOut)??;
            if response.status() == reqwest::StatusCode::NOT_MODIFIED {
                return Ok(OpdsConditionalCatalog::NotModified);
            }
            if !response.status().is_success() {
                return Err(OpdsError::HttpStatus(response.status()));
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
            let response = self.send(self.request(url).header(reqwest::header::ACCEPT, format!("{OPDS_2_MEDIA_TYPE}, application/atom+xml;profile=opds-catalog, application/atom+xml;q=0.9, text/html;q=0.2"))).await;
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

    fn request(&self, url: Url) -> RequestBuilder {
        self.http.get(url)
    }

    async fn send(&self, request: RequestBuilder) -> Result<Response, OpdsError> {
        let response = tokio::time::timeout(NETWORK_IDLE_TIMEOUT, request.send()).await.map_err(|_| OpdsError::TimedOut)??;
        if !response.status().is_success() {
            return Err(OpdsError::HttpStatus(response.status()));
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
        OpdsError::HttpStatus(_) => "http_error",
        OpdsError::ResponseTooLarge { .. } => "response_too_large",
        OpdsError::UnsupportedCatalogFormat => "unsupported_catalog_format",
        OpdsError::InvalidCatalog(_) => "invalid_catalog",
        OpdsError::NonFreeAcquisition => "non_free_acquisition",
        OpdsError::SearchUnavailable => "search_unavailable",
        OpdsError::InvalidSearchTemplate(_) => "invalid_search_template",
        OpdsError::UnsupportedBookResponse(_) => "unsupported_book_response",
        OpdsError::Io(_) => "io_error",
    }
}

#[cfg(debug_assertions)]
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

pub struct DownloadedOpdsBook {
    path: PathBuf,
    file_name: String,
}

impl DownloadedOpdsBook {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
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
                    navigation.push(OpdsCatalogLink { title: nonempty(entry.title.as_str()).unwrap_or("Untitled").to_owned(), url: url.to_string(), target, number_of_items: None, active: false });
                }
            }
            continue;
        }
        let title = nonempty(entry.title.as_str()).unwrap_or("Untitled").to_owned();
        let acquisitions =
            acquisition_links.into_iter().enumerate().filter_map(|(link_index, link)| acquisition_from_atom(&base_url, link, &title, acquisition_details.get(entry_index).and_then(|links| links.get(link_index)))).collect::<Vec<_>>();
        let image_url = entry.links.iter().filter(|link| is_image_relation(&link.rel)).find_map(|link| resolve_url(&base_url, &link.href).ok()).map(|url| url.to_string());
        let description = atom_description(entry);
        let preferred_download = preferred_download(&acquisitions);
        let authors = entry
            .authors
            .iter()
            .filter_map(|author| {
                let name = nonempty(&author.name)?.to_owned();
                let raw_uri = author.uri.as_deref().and_then(nonempty);
                let resolved_uri = raw_uri.and_then(|uri| resolve_url(&base_url, uri).ok()).map(|url| url.to_string());
                let identifier = resolved_uri.clone().or_else(|| raw_uri.map(str::to_owned));
                Some(OpdsAuthorCredit { name, sort_name: None, identifiers: identifier.into_iter().collect(), provider_link: resolved_uri.map(|url| OpdsAuthorLink { url, target: OpdsLinkTarget::Web }) })
            })
            .collect::<Vec<_>>();
        let self_url = relation_url_atom(&base_url, &entry.links, "self");
        let id = stable_publication_id(Some(&entry.id), self_url.as_deref(), &acquisitions, &base_url, &title, &authors);
        let languages = atom_publication_languages(entry);
        publications.push(OpdsPublication { work_id: work_id_from_publication_id(&id), id, title, authors, languages, description, image_url, preferred_download, acquisitions });
    }
    let facets = parse_opds_1_facets(&base_url, bytes)?;
    let search = feed.links.iter().find(|link| link.rel == "search").and_then(|link| search_from_atom(&base_url, link));
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
        search,
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
                let link = OpdsCatalogLink { title, url: url.to_string(), target: OpdsLinkTarget::Catalog, number_of_items, active };
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
            if repaired.as_bytes() == bytes {
                return Err(OpdsError::InvalidCatalog(original_error.to_string()));
            }
            parse(repaired.as_bytes()).map_err(|_| OpdsError::InvalidCatalog(original_error.to_string()))
        }
    }
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
        search: links.iter().find(|link| json_link_has_relation(link, "search")).and_then(|link| search_from_json(&source_url, link)),
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
    let image_url = object.get("images").and_then(Value::as_array).and_then(|images| images.iter().find_map(|image| json_link_url(base, image))).map(|url| url.to_string());
    let mut acquisitions = links.iter().filter_map(|link| acquisition_from_json(base, link, &title)).collect::<Vec<_>>();
    if acquisitions.is_empty() {
        if let Some(url) = links.iter().find(|link| json_link_has_relation(link, "alternate") && link.get("type").and_then(Value::as_str).is_some_and(|media_type| media_type == "text/html")).and_then(|link| json_link_url(base, link)) {
            acquisitions.push(acquisition(url, OpdsAcquisitionKind::Generic, Some("text/html"), &title, None, Vec::new()));
        }
    }
    let self_url = relation_url_json(base, &links, "self");
    let id = stable_publication_id(metadata.get("identifier").and_then(Value::as_str), self_url.as_deref(), &acquisitions, base, &title, &authors);
    let preferred_download = preferred_download(&acquisitions);
    Some(OpdsPublication { work_id: work_id_from_publication_id(&id), id, title, authors, languages, description, image_url, preferred_download, acquisitions })
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
    let value = value.trim().replace('_', "-").to_ascii_lowercase();
    let valid = !value.is_empty()
        && value.len() <= 63
        && value.split('-').all(|part| !part.is_empty() && part.len() <= 8 && part.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        && value.split('-').next().is_some_and(|part| ((2..=8).contains(&part.len()) || part == "x") && part.bytes().all(|byte| byte.is_ascii_alphabetic()));
    valid.then_some(value)
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
        DescriptionFormat::Html => rich_description(value, |markup, safe_html, plain_text| OpdsDescription::Html { markup, safe_html, plain_text }),
        DescriptionFormat::Xhtml => rich_description(value, |markup, safe_html, plain_text| OpdsDescription::Xhtml { markup, safe_html, plain_text }),
    }
}

fn rich_description(value: &str, description: impl FnOnce(String, String, String) -> OpdsDescription) -> Option<OpdsDescription> {
    let plain_text = compact_description_whitespace(&html::parse::plain_text_from_fragment(value))?;
    Some(description(value.to_owned(), sanitized_description_markup(value), plain_text))
}

fn sanitized_description_markup(value: &str) -> String {
    let tags = HashSet::from([
        "abbr",
        "b",
        "blockquote",
        "br",
        "cite",
        "code",
        "dd",
        "del",
        "dfn",
        "div",
        "dl",
        "dt",
        "em",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "i",
        "ins",
        "kbd",
        "li",
        "mark",
        "ol",
        "p",
        "pre",
        "q",
        "s",
        "samp",
        "small",
        "span",
        "strike",
        "strong",
        "sub",
        "sup",
        "time",
        "tt",
        "u",
        "ul",
        "var",
        "wbr",
    ]);
    ammonia::Builder::default().tags(tags).generic_attributes(HashSet::new()).tag_attributes(HashMap::new()).clean(value).to_string()
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
        Value::String(name) => nonempty(name).map(|name| vec![OpdsAuthorCredit { name: name.to_owned(), sort_name: None, identifiers: Vec::new(), provider_link: None }]).unwrap_or_default(),
        Value::Object(author) => {
            let Some(name) = author.get("name").and_then(localized_text).and_then(|name| nonempty(&name).map(str::to_owned)) else {
                return Vec::new();
            };
            let sort_name = author.get("sortAs").and_then(localized_text).and_then(|name| nonempty(&name).map(str::to_owned));
            let identifiers = author.get("identifier").map(identifier_values).unwrap_or_default();
            let provider_link = author.get("links").and_then(Value::as_array).and_then(|links| links.iter().find_map(|link| contributor_link(base, link)));
            vec![OpdsAuthorCredit { name, sort_name, identifiers, provider_link }]
        }
        Value::Array(authors) => authors.iter().flat_map(|author| contributor_credits(base, author)).collect(),
        _ => Vec::new(),
    }
}

fn identifier_values(value: &Value) -> Vec<String> {
    match value {
        Value::String(identifier) => nonempty(identifier).map(|identifier| vec![identifier.to_owned()]).unwrap_or_default(),
        Value::Array(identifiers) => identifiers.iter().flat_map(identifier_values).collect(),
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
        Value::Object(variants) => variants.get("en").and_then(Value::as_str).or_else(|| variants.values().find_map(Value::as_str)).map(str::to_owned),
        _ => None,
    }
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
    Some(OpdsCatalogLink { title, url: url.to_string(), target, number_of_items, active })
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

fn search_from_atom(base: &Url, link: &AtomLink) -> Option<OpdsSearch> {
    if link.mime_type.as_deref().map(normalized_media_type).as_deref() == Some(OPENSEARCH_MEDIA_TYPE) {
        let url = resolve_url(base, &link.href).ok()?;
        Some(OpdsSearch::OpenSearchDescription { url: url.to_string() })
    } else if link.href.contains('{') {
        Some(OpdsSearch::UriTemplate { base_url: base.to_string(), template: link.href.clone() })
    } else {
        None
    }
}

fn search_from_json(base: &Url, value: &Value) -> Option<OpdsSearch> {
    let object = value.as_object()?;
    let template = object.get("href")?.as_str()?;
    if !object.get("templated").and_then(Value::as_bool).unwrap_or(template.contains('{')) {
        return None;
    }
    Some(OpdsSearch::UriTemplate { base_url: base.to_string(), template: template.to_owned() })
}

fn parse_open_search_template(bytes: &[u8]) -> Result<String, OpdsError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) if element.local_name().as_ref() == b"Url" => {
                let mut media_type = None;
                let mut template = None;
                for attribute in element.attributes().with_checks(false) {
                    let attribute = attribute.map_err(|error| OpdsError::InvalidCatalog(error.to_string()))?;
                    let value = attribute.decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder()).map_err(|error| OpdsError::InvalidCatalog(error.to_string()))?.into_owned();
                    match attribute.key.local_name().as_ref() {
                        b"type" => media_type = Some(value),
                        b"template" => template = Some(value),
                        _ => {}
                    }
                }
                if media_type.as_deref().is_some_and(is_catalog_media_type) {
                    if let Some(template) = template {
                        return Ok(template);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(OpdsError::InvalidCatalog(error.to_string())),
        }
    }
    Err(OpdsError::InvalidCatalog("OpenSearch description has no OPDS URL template".to_owned()))
}

fn validate_search_query(query: &OpdsSearchQuery) -> Result<(), OpdsError> {
    for (name, value) in [("query", Some(query.query.as_str())), ("title", query.title.as_deref()), ("author", query.author.as_deref())] {
        if value.is_some_and(|value| value.len() > 500 || value.chars().any(char::is_control)) {
            return Err(OpdsError::InvalidSearchTemplate(format!("{name} search value must be at most 500 characters without control characters")));
        }
    }
    if query.query.trim().is_empty() && query.title.as_deref().is_none_or(|value| value.trim().is_empty()) && query.author.as_deref().is_none_or(|value| value.trim().is_empty()) {
        return Err(OpdsError::InvalidSearchTemplate("at least one search value is required".to_owned()));
    }
    Ok(())
}

fn expand_search_template(template: &str, query: &OpdsSearchQuery) -> Result<String, OpdsError> {
    let normalized = remove_optional_open_search_parameters(template);
    let template = UriTemplate::new(normalized).map_err(|error| OpdsError::InvalidSearchTemplate(error.to_string()))?;
    let mut values = HashValues::new();
    values.insert("query", query.query.as_str());
    values.insert("searchTerms", query.query.as_str());
    if let Some(title) = query.title.as_deref().filter(|value| !value.trim().is_empty()) {
        values.insert("title", title);
    }
    if let Some(author) = query.author.as_deref().filter(|value| !value.trim().is_empty()) {
        values.insert("author", author);
    }
    template.expand(&values).map_err(|error| OpdsError::InvalidSearchTemplate(error.to_string()))
}

fn remove_optional_open_search_parameters(template: &str) -> String {
    let mut result = template.to_owned();
    for parameter in ["count", "startIndex", "startPage", "language", "inputEncoding", "outputEncoding"] {
        result = result.replace(&format!("{{{parameter}?}}"), "");
    }
    result
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
    let url = base.join(value).map_err(|error| OpdsError::InvalidUrl(error.to_string()))?;
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
        FB2_MEDIA_TYPE | "application/fb2+xml" => Some(OpdsBookFormat::Fb2),
        CBZ_MEDIA_TYPE | "application/x-cbz" => Some(OpdsBookFormat::Cbz),
        WEBPUB_MEDIA_TYPE => Some(OpdsBookFormat::WebPub),
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
    } else if path.ends_with(".fb2") {
        Some(OpdsBookFormat::Fb2)
    } else if path.ends_with(".cbz") {
        Some(OpdsBookFormat::Cbz)
    } else if path.ends_with(".webpub") {
        Some(OpdsBookFormat::WebPub)
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
      "navigation":[{"title":"Recent","href":"recent","type":"application/opds+json"}],
      "publications":[{"metadata":{"identifier":"urn:isbn:1","title":{"en":"A Book"},"author":["One",{"name":"Two","sortAs":"Two, Author","identifier":"https://isni.org/isni/0000000121400562","links":[{"href":"authors/two","type":"application/opds+json"}]}]},"links":[{"rel":"download","href":"../book.epub","type":"application/epub+zip"}],"images":[{"href":"cover.jpg","type":"image/jpeg"}]}]
    }"#;

    #[test]
    fn built_in_endpoints_are_complete_and_safe() {
        let endpoints = built_in_opds_endpoints();
        assert_eq!(endpoints.len(), 8);
        assert_eq!(endpoints.iter().map(|endpoint| endpoint.id.as_str()).collect::<HashSet<_>>().len(), endpoints.len());
        assert!(endpoints.iter().all(|endpoint| !endpoint.title.trim().is_empty() && !endpoint.description.trim().is_empty() && !endpoint.languages.is_empty()));
        let open_library = endpoints.iter().find(|endpoint| endpoint.id.as_str() == "open-library").unwrap();
        assert_eq!(open_library.url, "https://openlibrary.org/opds/");
        assert!(endpoints.iter().all(|endpoint| parse_http_url(&endpoint.url).is_ok()));
    }

    #[test]
    fn performance_log_urls_exclude_queries_and_fragments() {
        assert_eq!(profile_log_url("https://catalog.example/search?q=private#result"), "https://catalog.example/search");
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
    async fn open_library_browse_search_and_acquisitions_are_live_and_parseable() {
        let client = OpdsClient::new().unwrap();
        let root = client.fetch_catalog("https://openlibrary.org/opds/").await.unwrap();
        assert_eq!(root.version, OpdsVersion::V2_0);
        assert_eq!(root.title, "Open Library");
        assert!(!root.navigation.is_empty());
        assert!(root.groups.iter().flat_map(|group| &group.publications).any(|publication| publication.id.starts_with("https://openlibrary.org/opds/books/") && !publication.acquisitions.is_empty()));
        let navigation = client.fetch_catalog(&root.navigation[0].url).await.unwrap();
        assert!(!navigation.publications.is_empty() || !navigation.groups.is_empty());
        let search = root.search.as_ref().unwrap();
        let results = client.search_definition(search, &OpdsSearchQuery { query: "Pride and Prejudice".to_owned(), title: None, author: None }).await.unwrap();
        assert!(!results.publications.is_empty() || !results.groups.is_empty());
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
    fn preserves_non_http_opds_1_person_identifiers_without_exposing_them_as_links() {
        let feed = OPDS_1.replace("<uri>authors/an-author</uri>", "<uri>urn:isni:0000000121400562</uri>");
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), Some("application/atom+xml"), feed.as_bytes()).unwrap();
        let author = &catalog.publications[0].authors[0];
        assert_eq!(author.identifiers, vec!["urn:isni:0000000121400562"]);
        assert_eq!(author.provider_link, None);
    }

    #[test]
    fn repairs_html_named_entities_in_legacy_atom_xhtml() {
        let feed = OPDS_1.replace("<summary>A summary</summary>", r#"<content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>&alpha; &copy; &amp; XML</p></div></content>"#);
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), Some("application/atom+xml"), feed.as_bytes()).unwrap();
        assert_eq!(catalog.publications.len(), 1);
        let description = catalog.publications[0].description.as_ref().unwrap();
        assert_eq!(description.plain_text(), "α © & XML");
        assert!(matches!(description, OpdsDescription::Xhtml { .. }));
        assert!(description.markup().is_some_and(|markup| markup.contains("<p>")));
    }

    #[test]
    fn normalizes_gutenberg_editions_to_one_work_and_preserves_description_markup() {
        let feed = OPDS_1
            .replace("<id>book-1</id>", "<id>urn:gutenberg:2465:3</id>")
            .replace("<summary>A summary</summary>", r#"<content type="xhtml"><div xmlns="http://www.w3.org/1999/xhtml"><p>This edition has images.</p><p>Title: Carmen</p></div></content>"#);
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), Some("application/atom+xml"), feed.as_bytes()).unwrap();
        assert_eq!(catalog.publications[0].work_id.as_deref(), Some("urn:gutenberg:2465"));
        let description = catalog.publications[0].description.as_ref().unwrap();
        assert_eq!(description.plain_text(), "This edition has images. Title: Carmen");
        assert!(matches!(description, OpdsDescription::Xhtml { .. }));
        assert!(description.markup().is_some_and(|markup| markup.contains("This edition has images.")));
    }

    #[test]
    fn sanitizes_rich_descriptions_before_exposing_them_to_the_webview() {
        let source =
            r#"<div class="layout"><p onclick="attack()"><strong>Safe formatting</strong></p><a href="javascript:attack()">unsafe link</a><img src="https://tracker.example/pixel" onerror="attack()"><script>attack()</script></div>"#;
        let description = typed_description(source, DescriptionFormat::Html).unwrap();
        let safe_html = description.safe_html().unwrap();
        assert!(safe_html.contains("<strong>Safe formatting</strong>"));
        assert!(safe_html.contains("unsafe link"));
        assert!(!safe_html.contains("onclick"));
        assert!(!safe_html.contains("javascript:"));
        assert!(!safe_html.contains("<img"));
        assert!(!safe_html.contains("<script"));
        assert!(!safe_html.contains("attack()</script>"));
        assert!(description.markup().is_some_and(|markup| markup.contains("onclick")));
        let serialized = serde_json::to_string(&description).unwrap();
        assert!(!serialized.contains("markup"));
        assert!(!serialized.contains("onclick"));
        assert!(serialized.contains("safe_html"));
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
    fn parses_opds_2_groups_navigation_pagination_and_search() {
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/v2/").unwrap(), Some(OPDS_2_MEDIA_TYPE), OPDS_2.as_bytes()).unwrap();
        assert_eq!(catalog.version, OpdsVersion::V2_0);
        assert_eq!(catalog.pagination.number_of_items, Some(1));
        assert_eq!(catalog.navigation[0].url, "https://example.com/catalog/v2/recent");
        assert_eq!(catalog.publications[0].authors.iter().map(|author| author.name.as_str()).collect::<Vec<_>>(), vec!["One", "Two"]);
        assert_eq!(catalog.publications[0].authors[1].sort_name.as_deref(), Some("Two, Author"));
        assert_eq!(catalog.publications[0].authors[1].identifiers, vec!["https://isni.org/isni/0000000121400562"]);
        assert_eq!(catalog.publications[0].authors[1].provider_link, Some(OpdsAuthorLink { url: "https://example.com/catalog/v2/authors/two".to_owned(), target: OpdsLinkTarget::Catalog }));
        let OpdsAcquisition::Direct(acquisition) = &catalog.publications[0].acquisitions[0] else { panic!("expected direct EPUB") };
        assert_eq!(acquisition.url, "https://example.com/catalog/book.epub");
        assert_eq!(catalog.publications[0].preferred_download.as_ref(), Some(acquisition));
        let OpdsSearch::UriTemplate { template, .. } = catalog.search.unwrap() else { panic!("expected URI template") };
        assert_eq!(expand_search_template(&template, &OpdsSearchQuery { query: "a & b".to_owned(), title: None, author: None }).unwrap(), "search?query=a%20%26%20b");
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
    fn advanced_search_expands_only_supported_template_variables() {
        let query = OpdsSearchQuery { query: "history".to_owned(), title: Some("A title".to_owned()), author: Some("An author".to_owned()) };
        assert_eq!(expand_search_template("search{?query,title,author}", &query).unwrap(), "search?query=history&title=A%20title&author=An%20author");
        assert!(validate_search_query(&OpdsSearchQuery::default()).is_err());
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
        assert!(endpoints_from_registry(local.clone()).is_err());
        assert_eq!(custom_endpoints_from_registry(local).unwrap()[0].url, "http://127.0.0.1:8080/opds");

        let legacy_path = temporary.path().join("legacy-custom-catalogs.json");
        std::fs::write(
            &legacy_path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "revision": 1,
                "catalogs": [{
                    "id": "legacy-local",
                    "title": "Legacy local",
                    "description": "Legacy custom catalog",
                    "languages": ["English"],
                    "url": "http://127.0.0.1:8081/opds",
                    "access": "open",
                    "authentication": { "kind": "none" }
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let (migrated, endpoints) = load_custom_registry_cache(&legacy_path).unwrap().unwrap();
        assert_eq!(migrated.schema_version, CATALOG_REGISTRY_SCHEMA_VERSION);
        assert_eq!(endpoints[0].id.as_str(), "legacy-local");
    }

    #[test]
    fn endpoint_store_persists_custom_endpoints_and_keeps_bundled_fallbacks() {
        let temporary = tempfile::tempdir().unwrap();
        let curated_path = temporary.path().join("curated.json");
        let custom_path = temporary.path().join("custom.json");
        let project_gutenberg = built_in_opds_endpoints().into_iter().find(|endpoint| endpoint.id.as_str() == "project-gutenberg").unwrap();
        save_registry_cache(&curated_path, &registry_from_endpoints(2, &[project_gutenberg])).unwrap();

        let mut store = OpdsEndpointStore::load(curated_path.clone(), custom_path.clone());
        assert!(store.endpoints().iter().any(|endpoint| endpoint.id.as_str() == "open-library"));
        let custom = store.add_custom_endpoint("Local books", "http://127.0.0.1:8080/opds").unwrap();
        assert!(custom.id.as_str().starts_with("custom-"));
        assert!(store.add_custom_endpoint("Duplicate", "http://127.0.0.1:8080/opds").unwrap_err().to_string().contains("already configured"));

        drop(store);
        let mut restored = OpdsEndpointStore::load(curated_path, custom_path);
        assert_eq!(restored.custom_endpoints(), &[custom.clone()]);
        restored.remove_custom_endpoint(&custom.id).unwrap();
        assert!(restored.custom_endpoints().is_empty());
    }

    #[test]
    fn endpoint_store_enforces_curated_registry_revisions() {
        let temporary = tempfile::tempdir().unwrap();
        let curated_path = temporary.path().join("curated.json");
        let custom_path = temporary.path().join("custom.json");
        let mut store = OpdsEndpointStore::load(curated_path, custom_path);
        let endpoint = OpdsEndpoint { id: OpdsEndpointId::parse("example").unwrap(), title: "Example".to_owned(), description: "Books".to_owned(), languages: vec!["English".to_owned()], url: "https://example.com/opds".to_owned() };
        store.replace_curated_registry(registry_from_endpoints(2, std::slice::from_ref(&endpoint))).unwrap();
        let changed = OpdsEndpoint { title: "Changed".to_owned(), ..endpoint.clone() };
        assert!(store.replace_curated_registry(registry_from_endpoints(2, &[changed])).unwrap_err().to_string().contains("without a revision increment"));
        assert!(store.replace_curated_registry(registry_from_endpoints(1, &[endpoint])).unwrap_err().to_string().contains("refusing stale"));
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
    fn parses_opensearch_description_and_sanitizes_file_names() {
        let description = br#"<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/"><Url type="application/atom+xml;profile=opds-catalog" template="/search?q={searchTerms}"/></OpenSearchDescription>"#;
        assert_eq!(parse_open_search_template(description).unwrap(), "/search?q={searchTerms}");
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
