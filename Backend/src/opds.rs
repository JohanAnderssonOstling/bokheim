use atom_syndication::{Feed, Link as AtomLink};
use futures_util::StreamExt;
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};
use reqwest::{Client, RequestBuilder, Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use uri_template::{HashValues, UriTemplate};

const MAX_CATALOG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SEARCH_DESCRIPTION_BYTES: u64 = 1024 * 1024;
const NETWORK_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const EPUB_MEDIA_TYPE: &str = "application/epub+zip";
const OPDS_2_MEDIA_TYPE: &str = "application/opds+json";
const OPENSEARCH_MEDIA_TYPE: &str = "application/opensearchdescription+xml";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpdsEndpointId {
    ProjectGutenberg,
    StandardEbooks,
    StandardEbooksNewReleases,
    FeedbooksPublicDomain,
    UnglueIt,
    TextosInfo,
    EbooksGratuits,
    OfficeOfTheHistorian,
    Gallica,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpdsEndpointAuthentication {
    None,
    Basic { username_label: String, password: OpdsPasswordRequirement },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpdsPasswordRequirement {
    Required,
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpdsEndpointAccess {
    Open,
    Membership,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct OpdsEndpoint {
    pub id: OpdsEndpointId,
    pub title: String,
    pub description: String,
    pub languages: Vec<String>,
    pub url: String,
    pub access: OpdsEndpointAccess,
    pub authentication: OpdsEndpointAuthentication,
}

impl OpdsEndpointId {
    pub fn endpoint(self) -> OpdsEndpoint {
        match self {
            Self::ProjectGutenberg => open_endpoint(self, "Project Gutenberg", "A large multilingual collection of free ebooks, with browsing and catalog search.", &["Multilingual"], "https://www.gutenberg.org/ebooks/search.opds/"),
            Self::StandardEbooks => membership_endpoint(self, "Standard Ebooks", "Carefully produced public-domain ebooks. Feed access requires an eligible Standard Ebooks account.", &["English"], "https://standardebooks.org/feeds/opds"),
            Self::StandardEbooksNewReleases => membership_endpoint(self, "Standard Ebooks — New Releases", "The latest carefully produced Standard Ebooks releases.", &["English"], "https://standardebooks.org/feeds/opds/new-releases"),
            Self::FeedbooksPublicDomain => open_endpoint(self, "Feedbooks Public Domain", "Curated public-domain classics available in modern ebook formats.", &["Multilingual"], "https://catalog.feedbooks.com/catalog/index.json"),
            Self::UnglueIt => open_endpoint(self, "Unglue.it", "Open and Creative Commons ebooks collected by the Free Ebook Foundation.", &["Multilingual"], "https://unglue.it/api/opds/"),
            Self::TextosInfo => open_endpoint(self, "textos.info", "A Spanish-language catalog of freely available literature.", &["Spanish"], "https://www.textos.info/catalogo.atom"),
            Self::EbooksGratuits => open_endpoint(self, "Ebooks Gratuits", "French-language public-domain and freely distributed ebooks.", &["French"], "https://www.ebooksgratuits.com/opds/index.php"),
            Self::OfficeOfTheHistorian => open_endpoint(self, "Office of the Historian", "The official Foreign Relations of the United States ebook collection.", &["English"], "https://history.state.gov/api/v1/catalog"),
            Self::Gallica => open_endpoint(self, "Gallica", "Public-domain EPUBs from the digital library of the Bibliothèque nationale de France.", &["French"], "https://gallica.bnf.fr/opds"),
        }
    }
}

pub fn built_in_opds_endpoints() -> Vec<OpdsEndpoint> {
    vec![
        OpdsEndpointId::ProjectGutenberg.endpoint(),
        OpdsEndpointId::StandardEbooks.endpoint(),
        OpdsEndpointId::StandardEbooksNewReleases.endpoint(),
        OpdsEndpointId::FeedbooksPublicDomain.endpoint(),
        OpdsEndpointId::UnglueIt.endpoint(),
        OpdsEndpointId::TextosInfo.endpoint(),
        OpdsEndpointId::EbooksGratuits.endpoint(),
        OpdsEndpointId::OfficeOfTheHistorian.endpoint(),
        OpdsEndpointId::Gallica.endpoint(),
    ]
}

fn open_endpoint(id: OpdsEndpointId, title: &str, description: &str, languages: &[&str], url: &str) -> OpdsEndpoint {
    OpdsEndpoint {
        id,
        title: title.to_owned(),
        description: description.to_owned(),
        languages: languages.iter().map(|language| (*language).to_owned()).collect(),
        url: url.to_owned(),
        access: OpdsEndpointAccess::Open,
        authentication: OpdsEndpointAuthentication::None,
    }
}

fn membership_endpoint(id: OpdsEndpointId, title: &str, description: &str, languages: &[&str], url: &str) -> OpdsEndpoint {
    OpdsEndpoint {
        id,
        title: title.to_owned(),
        description: description.to_owned(),
        languages: languages.iter().map(|language| (*language).to_owned()).collect(),
        url: url.to_owned(),
        access: OpdsEndpointAccess::Membership,
        authentication: OpdsEndpointAuthentication::Basic { username_label: "Patrons Circle email address".to_owned(), password: OpdsPasswordRequirement::Empty },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpdsVersion {
    V1_2,
    V2_0,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct OpdsCatalogLink {
    pub title: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpdsSearch {
    UriTemplate { base_url: String, template: String },
    OpenSearchDescription { url: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Type)]
pub struct OpdsPagination {
    pub first: Option<String>,
    pub previous: Option<String>,
    pub next: Option<String>,
    pub last: Option<String>,
    pub number_of_items: Option<u32>,
    pub items_per_page: Option<u32>,
    pub current_page: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsEpubAcquisition {
    pub url: String,
    pub kind: OpdsAcquisitionKind,
    pub suggested_file_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct OpdsUnsupportedAcquisition {
    pub url: String,
    pub kind: OpdsAcquisitionKind,
    pub media_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum OpdsAcquisition {
    Epub(OpdsEpubAcquisition),
    Unsupported(OpdsUnsupportedAcquisition),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct OpdsPublication {
    pub id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub free_epub: Option<OpdsEpubAcquisition>,
    pub acquisitions: Vec<OpdsAcquisition>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct OpdsGroup {
    pub title: String,
    pub navigation: Vec<OpdsCatalogLink>,
    pub publications: Vec<OpdsPublication>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct OpdsFacet {
    pub title: String,
    pub links: Vec<OpdsCatalogLink>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct OpdsCatalog {
    pub version: OpdsVersion,
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

#[derive(Clone)]
pub struct OpdsCredentials {
    username: String,
    password: String,
    origin: String,
}

impl OpdsCredentials {
    pub fn for_endpoint(endpoint: OpdsEndpointId, username: impl Into<String>, password: impl Into<String>) -> Result<Self, OpdsError> {
        Self::for_url(&endpoint.endpoint().url, username, password)
    }

    fn for_url(url: &str, username: impl Into<String>, password: impl Into<String>) -> Result<Self, OpdsError> {
        let username = username.into();
        if username.is_empty() || username.chars().any(char::is_control) {
            return Err(OpdsError::InvalidCredentials);
        }
        let url = parse_http_url(url)?;
        Ok(Self { username, password: password.into(), origin: url.origin().ascii_serialization() })
    }

    fn applies_to(&self, url: &Url) -> bool {
        self.origin == url.origin().ascii_serialization()
    }
}

impl fmt::Debug for OpdsCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("OpdsCredentials").field("username", &self.username).field("password", &"[redacted]").field("origin", &self.origin).finish()
    }
}

#[derive(Debug)]
pub enum OpdsError {
    InvalidUrl(String),
    InvalidCredentials,
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
            Self::InvalidUrl(error) => write!(formatter, "invalid OPDS URL: {error}"),
            Self::InvalidCredentials => formatter.write_str("OPDS username must be non-empty and contain no control characters"),
            Self::Network(error) => write!(formatter, "OPDS network request failed: {error}"),
            Self::TimedOut => formatter.write_str("OPDS server did not respond before the timeout"),
            Self::HttpStatus(status) => write!(formatter, "OPDS server returned HTTP {status}"),
            Self::ResponseTooLarge { limit } => write!(formatter, "OPDS response exceeds the {limit}-byte limit"),
            Self::UnsupportedCatalogFormat => formatter.write_str("response is neither an OPDS 1.2 Atom feed nor an OPDS 2.0 JSON feed"),
            Self::InvalidCatalog(error) => write!(formatter, "invalid OPDS catalog: {error}"),
            Self::NonFreeAcquisition => formatter.write_str("only open-access or generic EPUB acquisitions can be imported directly"),
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

#[derive(Clone)]
pub struct OpdsClient {
    http: Client,
}

impl OpdsClient {
    pub fn new() -> Result<Self, OpdsError> {
        let http = Client::builder().connect_timeout(Duration::from_secs(10)).redirect(reqwest::redirect::Policy::limited(10)).user_agent(format!("{}/{} OPDS", crate::APP_NAME, env!("CARGO_PKG_VERSION"))).build()?;
        Ok(Self { http })
    }

    pub async fn fetch_catalog(&self, url: &str, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, OpdsError> {
        let url = parse_http_url(url)?;
        self.fetch_catalog_url(url, credentials).await
    }

    pub async fn search(&self, catalog: &OpdsCatalog, query: &str, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, OpdsError> {
        let search = catalog.search.as_ref().ok_or(OpdsError::SearchUnavailable)?;
        self.search_definition(search, query, credentials).await
    }

    pub async fn search_definition(&self, search: &OpdsSearch, query: &str, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, OpdsError> {
        let (base_url, template) = match search {
            OpdsSearch::UriTemplate { base_url, template } => (parse_http_url(base_url)?, template.clone()),
            OpdsSearch::OpenSearchDescription { url } => {
                let description_url = parse_http_url(url)?;
                let response = self.send(self.request(description_url.clone(), credentials)).await?;
                let final_url = validated_response_url(&response)?;
                let bytes = read_bounded(response, MAX_SEARCH_DESCRIPTION_BYTES).await?;
                (final_url, parse_open_search_template(&bytes)?)
            }
        };
        let expanded = expand_search_template(&template, query)?;
        let url = resolve_url(&base_url, &expanded)?;
        self.fetch_catalog_url(url, credentials).await
    }

    pub(crate) async fn download_epub(&self, acquisition: &OpdsEpubAcquisition, credentials: Option<&OpdsCredentials>, temporary_directory: &Path) -> Result<DownloadedOpdsEpub, OpdsError> {
        if !matches!(acquisition.kind, OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic) {
            return Err(OpdsError::NonFreeAcquisition);
        }
        let url = parse_http_url(&acquisition.url)?;
        let response = self.send(self.request(url, credentials).header(reqwest::header::ACCEPT, EPUB_MEDIA_TYPE)).await?;
        let _ = validated_response_url(&response)?;
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(normalized_media_type);
        if media_type.as_deref().is_some_and(|value| !matches!(value, EPUB_MEDIA_TYPE | "application/octet-stream" | "application/zip" | "binary/octet-stream")) {
            return Err(OpdsError::UnsupportedBookResponse(media_type));
        }
        if response.content_length().is_some_and(|length| length > crate::transfer::MAX_BOOK_BYTES) {
            return Err(OpdsError::ResponseTooLarge { limit: crate::transfer::MAX_BOOK_BYTES });
        }
        tokio::fs::create_dir_all(temporary_directory).await?;
        let path = temporary_directory.join(format!("opds-{}.epub", uuid::Uuid::new_v4()));
        let guard = DownloadedOpdsEpub { path, file_name: acquisition.suggested_file_name.clone() };
        let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&guard.path).await?;
        let mut stream = response.bytes_stream();
        let mut downloaded = 0_u64;
        while let Some(chunk) = tokio::time::timeout(NETWORK_IDLE_TIMEOUT, stream.next()).await.map_err(|_| OpdsError::TimedOut)? {
            let chunk = chunk?;
            downloaded = downloaded.checked_add(chunk.len() as u64).ok_or(OpdsError::ResponseTooLarge { limit: crate::transfer::MAX_BOOK_BYTES })?;
            if downloaded > crate::transfer::MAX_BOOK_BYTES {
                return Err(OpdsError::ResponseTooLarge { limit: crate::transfer::MAX_BOOK_BYTES });
            }
            file.write_all(&chunk).await?;
        }
        if downloaded == 0 {
            return Err(OpdsError::InvalidCatalog("EPUB acquisition returned an empty response".to_owned()));
        }
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        Ok(guard)
    }

    async fn fetch_catalog_url(&self, url: Url, credentials: Option<&OpdsCredentials>) -> Result<OpdsCatalog, OpdsError> {
        let response = self.send(self.request(url, credentials).header(reqwest::header::ACCEPT, format!("{OPDS_2_MEDIA_TYPE}, application/atom+xml;profile=opds-catalog, application/atom+xml;q=0.9"))).await?;
        let final_url = validated_response_url(&response)?;
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).map(normalized_media_type);
        let bytes = read_bounded(response, MAX_CATALOG_BYTES).await?;
        parse_catalog(final_url, media_type.as_deref(), &bytes)
    }

    fn request(&self, url: Url, credentials: Option<&OpdsCredentials>) -> RequestBuilder {
        let use_credentials = credentials.is_some_and(|credentials| credentials.applies_to(&url));
        let request = self.http.get(url);
        match (credentials, use_credentials) {
            (Some(credentials), true) => request.basic_auth(&credentials.username, Some(&credentials.password)),
            _ => request,
        }
    }

    async fn send(&self, request: RequestBuilder) -> Result<Response, OpdsError> {
        let response = tokio::time::timeout(NETWORK_IDLE_TIMEOUT, request.send()).await.map_err(|_| OpdsError::TimedOut)??;
        if !response.status().is_success() {
            return Err(OpdsError::HttpStatus(response.status()));
        }
        Ok(response)
    }
}

pub(crate) struct DownloadedOpdsEpub {
    path: PathBuf,
    file_name: String,
}

impl DownloadedOpdsEpub {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn file_name(&self) -> &str {
        &self.file_name
    }
}

impl Drop for DownloadedOpdsEpub {
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
    let feed = Feed::read_from(Cursor::new(bytes)).map_err(|error| OpdsError::InvalidCatalog(error.to_string()))?;
    let base_url = match feed.base.as_deref() {
        Some(base) => resolve_url(&source_url, base)?,
        None => source_url.clone(),
    };
    let mut navigation = Vec::new();
    let mut publications = Vec::new();
    for entry in &feed.entries {
        let acquisition_links = entry.links.iter().filter(|link| acquisition_kind(&link.rel).is_some()).collect::<Vec<_>>();
        if acquisition_links.is_empty() {
            if let Some(link) = entry.links.iter().find(|link| link.mime_type.as_deref().is_some_and(is_catalog_media_type) || is_navigation_relation(&link.rel)) {
                if let Ok(url) = resolve_url(&base_url, &link.href) {
                    navigation.push(OpdsCatalogLink { title: nonempty(entry.title.as_str()).unwrap_or("Untitled").to_owned(), url: url.to_string() });
                }
            }
            continue;
        }
        let title = nonempty(entry.title.as_str()).unwrap_or("Untitled").to_owned();
        let acquisitions = acquisition_links.into_iter().filter_map(|link| acquisition_from_atom(&base_url, link, &title)).collect::<Vec<_>>();
        let image_url = entry.links.iter().filter(|link| is_image_relation(&link.rel)).find_map(|link| resolve_url(&base_url, &link.href).ok()).map(|url| url.to_string());
        let description = entry.summary.as_ref().map(|summary| summary.as_str()).or_else(|| entry.content.as_ref().and_then(|content| content.value())).and_then(nonempty).map(str::to_owned);
        let free_epub = free_epub_acquisition(&acquisitions);
        publications.push(OpdsPublication { id: entry.id.clone(), title, authors: entry.authors.iter().filter_map(|author| nonempty(&author.name).map(str::to_owned)).collect(), description, image_url, free_epub, acquisitions });
    }
    let facets = feed.links.iter().filter(|link| link.rel == "http://opds-spec.org/facet").filter_map(|link| catalog_link_from_atom(&base_url, link)).collect::<Vec<_>>();
    let search = feed.links.iter().find(|link| link.rel == "search").and_then(|link| search_from_atom(&base_url, link));
    Ok(OpdsCatalog {
        version: OpdsVersion::V1_2,
        title: nonempty(feed.title.as_str()).unwrap_or("Untitled catalog").to_owned(),
        source_url: source_url.to_string(),
        self_url: relation_url_atom(&base_url, &feed.links, "self"),
        start_url: relation_url_atom(&base_url, &feed.links, "start"),
        navigation,
        publications,
        groups: Vec::new(),
        facets: if facets.is_empty() { Vec::new() } else { vec![OpdsFacet { title: "Facets".to_owned(), links: facets }] },
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
    Some(OpdsGroup { title, navigation: parse_json_navigation(base, object.get("navigation")), publications: parse_json_publications(base, object.get("publications")) })
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
    let title = metadata.get("title").and_then(localized_text).unwrap_or_else(|| "Untitled".to_owned());
    let id = metadata.get("identifier").and_then(Value::as_str).unwrap_or(&title).to_owned();
    let authors = metadata.get("author").map(contributor_names).unwrap_or_default();
    let description = metadata.get("description").and_then(localized_text).and_then(|value| nonempty(&value).map(str::to_owned));
    let image_url = object.get("images").and_then(Value::as_array).and_then(|images| images.iter().find_map(|image| json_link_url(base, image))).map(|url| url.to_string());
    let acquisitions = object.get("links").and_then(Value::as_array).map(|links| links.iter().filter_map(|link| acquisition_from_json(base, link, &title)).collect::<Vec<_>>()).unwrap_or_default();
    let free_epub = free_epub_acquisition(&acquisitions);
    Some(OpdsPublication { id, title, authors, description, image_url, free_epub, acquisitions })
}

fn free_epub_acquisition(acquisitions: &[OpdsAcquisition]) -> Option<OpdsEpubAcquisition> {
    acquisitions.iter().find_map(|acquisition| match acquisition {
        OpdsAcquisition::Epub(epub) if matches!(epub.kind, OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic) => Some(epub.clone()),
        _ => None,
    })
}

fn contributor_names(value: &Value) -> Vec<String> {
    match value {
        Value::String(name) => nonempty(name).map(|name| vec![name.to_owned()]).unwrap_or_default(),
        Value::Object(author) => author.get("name").and_then(localized_text).into_iter().collect(),
        Value::Array(authors) => authors.iter().flat_map(contributor_names).collect(),
        _ => Vec::new(),
    }
}

fn localized_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Object(variants) => variants.get("en").and_then(Value::as_str).or_else(|| variants.values().find_map(Value::as_str)).map(str::to_owned),
        _ => None,
    }
}

fn acquisition_from_atom(base: &Url, link: &AtomLink, title: &str) -> Option<OpdsAcquisition> {
    let kind = acquisition_kind(&link.rel)?;
    let url = resolve_url(base, &link.href).ok()?;
    Some(acquisition(url, kind, link.mime_type.as_deref(), title))
}

fn acquisition_from_json(base: &Url, value: &Value, title: &str) -> Option<OpdsAcquisition> {
    let object = value.as_object()?;
    let kind = json_relations(value).find_map(acquisition_kind)?;
    let url = resolve_url(base, object.get("href")?.as_str()?).ok()?;
    Some(acquisition(url, kind, object.get("type").and_then(Value::as_str), title))
}

fn acquisition(url: Url, kind: OpdsAcquisitionKind, media_type: Option<&str>, title: &str) -> OpdsAcquisition {
    if media_type.map(normalized_media_type).as_deref() == Some(EPUB_MEDIA_TYPE) {
        OpdsAcquisition::Epub(OpdsEpubAcquisition { suggested_file_name: safe_epub_file_name(title, &url), url: url.to_string(), kind })
    } else {
        OpdsAcquisition::Unsupported(OpdsUnsupportedAcquisition { url: url.to_string(), kind, media_type: media_type.map(str::to_owned) })
    }
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

fn catalog_link_from_atom(base: &Url, link: &AtomLink) -> Option<OpdsCatalogLink> {
    let url = resolve_url(base, &link.href).ok()?;
    Some(OpdsCatalogLink { title: link.title.as_deref().and_then(nonempty).unwrap_or("Untitled").to_owned(), url: url.to_string() })
}

fn catalog_link_from_json(base: &Url, value: &Value) -> Option<OpdsCatalogLink> {
    let object = value.as_object()?;
    let url = resolve_url(base, object.get("href")?.as_str()?).ok()?;
    let title = object.get("title").and_then(localized_text).unwrap_or_else(|| "Untitled".to_owned());
    Some(OpdsCatalogLink { title, url: url.to_string() })
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

fn expand_search_template(template: &str, query: &str) -> Result<String, OpdsError> {
    let normalized = remove_optional_open_search_parameters(template);
    let template = UriTemplate::new(normalized).map_err(|error| OpdsError::InvalidSearchTemplate(error.to_string()))?;
    let mut values = HashValues::new();
    values.insert("query", query);
    values.insert("searchTerms", query);
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
    matches!(normalized_media_type(value).as_str(), OPDS_2_MEDIA_TYPE | "application/atom+xml")
}

fn is_navigation_relation(value: &str) -> bool {
    matches!(value, "subsection" | "http://opds-spec.org/sort/new" | "http://opds-spec.org/sort/popular")
}

fn is_image_relation(value: &str) -> bool {
    matches!(value, "http://opds-spec.org/image" | "http://opds-spec.org/image/thumbnail" | "cover" | "thumbnail")
}

fn safe_epub_file_name(title: &str, url: &Url) -> String {
    let url_stem = url.path_segments().and_then(Iterator::last).and_then(|segment| urlencoding::decode(segment).ok()).map(|segment| segment.into_owned()).filter(|segment| segment.to_ascii_lowercase().ends_with(".epub"));
    let candidate = url_stem.unwrap_or_else(|| format!("{}.epub", nonempty(title).unwrap_or("Untitled")));
    let mut sanitized = candidate.chars().map(|character| if character.is_control() || matches!(character, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { character }).collect::<String>();
    if !sanitized.to_ascii_lowercase().ends_with(".epub") {
        sanitized.push_str(".epub");
    }
    if sanitized.len() > 240 {
        sanitized.truncate(sanitized.floor_char_boundary(235));
        sanitized.push_str(".epub");
    }
    if sanitized == ".epub" {
        "Untitled.epub".to_owned()
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
    use crate::app_core::AppCore;
    use crate::home::AppDataLocation;
    use axum::body::Body;
    use axum::http::{header, HeaderMap, Response, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use std::collections::HashSet;
    use std::io::Write;
    use std::sync::Arc;
    use zip::write::SimpleFileOptions;

    const OPDS_1: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <id>catalog</id><title>Test catalog</title><updated>2024-01-01T00:00:00Z</updated>
  <link rel="self" href="/opds" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  <link rel="next" href="?page=2" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  <entry><id>book-1</id><title>A Book</title><updated>2024-01-01T00:00:00Z</updated><author><name>An Author</name></author><summary>A summary</summary><link rel="http://opds-spec.org/image/thumbnail" href="covers/1.jpg" type="image/jpeg"/><link rel="http://opds-spec.org/acquisition/open-access" href="books/1.epub" type="application/epub+zip"/></entry>
</feed>"#;

    const OPDS_2: &str = r#"{
      "metadata":{"title":"Test catalog","numberOfItems":1},
      "links":[{"rel":"self","href":"/v2","type":"application/opds+json"},{"rel":["next"],"href":"?page=2","type":"application/opds+json"},{"rel":"search","href":"search{?query,title}","type":"application/opds+json","templated":true}],
      "navigation":[{"title":"Recent","href":"recent","type":"application/opds+json"}],
      "publications":[{"metadata":{"identifier":"urn:isbn:1","title":{"en":"A Book"},"author":["One",{"name":"Two"}]},"links":[{"rel":"download","href":"../book.epub","type":"application/epub+zip"}],"images":[{"href":"cover.jpg","type":"image/jpeg"}]}]
    }"#;

    #[test]
    fn standard_ebooks_endpoints_are_complete_and_safe() {
        let endpoints = built_in_opds_endpoints();
        assert_eq!(endpoints.len(), 9);
        assert_eq!(endpoints.iter().map(|endpoint| endpoint.id).collect::<HashSet<_>>().len(), endpoints.len());
        assert!(endpoints.iter().all(|endpoint| !endpoint.title.trim().is_empty() && !endpoint.description.trim().is_empty() && !endpoint.languages.is_empty()));
        let standard_ebooks = endpoints.iter().find(|endpoint| endpoint.id == OpdsEndpointId::StandardEbooks).unwrap();
        assert_eq!(standard_ebooks.url, "https://standardebooks.org/feeds/opds");
        let new_releases = endpoints.iter().find(|endpoint| endpoint.id == OpdsEndpointId::StandardEbooksNewReleases).unwrap();
        assert_eq!(new_releases.url, "https://standardebooks.org/feeds/opds/new-releases");
        for endpoint in endpoints.iter().filter(|endpoint| endpoint.access == OpdsEndpointAccess::Membership) {
            assert_eq!(parse_http_url(&endpoint.url).unwrap().scheme(), "https");
            let OpdsEndpointAuthentication::Basic { username_label, password } = &endpoint.authentication else { panic!("Standard Ebooks requires Basic authentication") };
            assert_eq!(username_label, "Patrons Circle email address");
            assert_eq!(*password, OpdsPasswordRequirement::Empty);
        }
        assert!(endpoints.iter().all(|endpoint| parse_http_url(&endpoint.url).is_ok()));
        assert_eq!(endpoints.iter().filter(|endpoint| endpoint.access == OpdsEndpointAccess::Open).count(), 7);
    }

    #[test]
    fn credentials_hide_the_password_in_debug_output() {
        let credentials = OpdsCredentials::for_endpoint(OpdsEndpointId::StandardEbooks, "reader@example.com", "very-secret").unwrap();
        let output = format!("{credentials:?}");
        assert!(output.contains("reader@example.com"));
        assert!(!output.contains("very-secret"));
    }

    #[test]
    fn credentials_are_sent_only_to_their_exact_origin() {
        let client = OpdsClient::new().unwrap();
        let credentials = OpdsCredentials::for_url("https://catalog.example/root", "reader", "secret").unwrap();
        let same_origin = client.request(Url::parse("https://catalog.example/books").unwrap(), Some(&credentials)).build().unwrap();
        let other_origin = client.request(Url::parse("https://downloads.example/book.epub").unwrap(), Some(&credentials)).build().unwrap();
        assert!(same_origin.headers().contains_key(reqwest::header::AUTHORIZATION));
        assert!(!other_origin.headers().contains_key(reqwest::header::AUTHORIZATION));
    }

    #[tokio::test]
    #[ignore = "requires live third-party catalog servers"]
    async fn open_built_in_catalogs_are_live_and_parseable() {
        let client = OpdsClient::new().unwrap();
        for endpoint in built_in_opds_endpoints().into_iter().filter(|endpoint| endpoint.access == OpdsEndpointAccess::Open) {
            let catalog = client.fetch_catalog(&endpoint.url, None).await.unwrap_or_else(|error| panic!("{} failed: {error}", endpoint.title));
            assert!(!catalog.title.trim().is_empty(), "{} returned no title", endpoint.title);
            assert!(!catalog.navigation.is_empty() || !catalog.publications.is_empty() || !catalog.groups.is_empty(), "{} returned no browseable content", endpoint.title);
        }
    }

    #[test]
    fn parses_opds_1_and_resolves_relative_links() {
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), Some("application/atom+xml; profile=opds-catalog"), OPDS_1.as_bytes()).unwrap();
        assert_eq!(catalog.version, OpdsVersion::V1_2);
        assert_eq!(catalog.self_url.as_deref(), Some("https://example.com/opds"));
        assert_eq!(catalog.pagination.next.as_deref(), Some("https://example.com/catalog/root.xml?page=2"));
        assert_eq!(catalog.publications[0].authors, vec!["An Author"]);
        assert_eq!(catalog.publications[0].image_url.as_deref(), Some("https://example.com/catalog/covers/1.jpg"));
        let OpdsAcquisition::Epub(acquisition) = &catalog.publications[0].acquisitions[0] else { panic!("expected EPUB") };
        assert_eq!(acquisition.url, "https://example.com/catalog/books/1.epub");
        assert_eq!(catalog.publications[0].free_epub.as_ref(), Some(acquisition));
    }

    #[test]
    fn parses_opds_2_groups_navigation_pagination_and_search() {
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/v2/").unwrap(), Some(OPDS_2_MEDIA_TYPE), OPDS_2.as_bytes()).unwrap();
        assert_eq!(catalog.version, OpdsVersion::V2_0);
        assert_eq!(catalog.pagination.number_of_items, Some(1));
        assert_eq!(catalog.navigation[0].url, "https://example.com/catalog/v2/recent");
        assert_eq!(catalog.publications[0].authors, vec!["One", "Two"]);
        let OpdsAcquisition::Epub(acquisition) = &catalog.publications[0].acquisitions[0] else { panic!("expected EPUB") };
        assert_eq!(acquisition.url, "https://example.com/catalog/book.epub");
        assert_eq!(catalog.publications[0].free_epub.as_ref(), Some(acquisition));
        let OpdsSearch::UriTemplate { template, .. } = catalog.search.unwrap() else { panic!("expected URI template") };
        assert_eq!(expand_search_template(&template, "a & b").unwrap(), "search?query=a%20%26%20b");
    }

    #[test]
    fn paid_borrowed_and_preview_epubs_are_not_offered_as_free_downloads() {
        for relation in ["http://opds-spec.org/acquisition/buy", "http://opds-spec.org/acquisition/borrow", "http://opds-spec.org/acquisition/sample"] {
            let feed = OPDS_1.replace("http://opds-spec.org/acquisition/open-access", relation);
            let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), None, feed.as_bytes()).unwrap();
            assert!(catalog.publications[0].free_epub.is_none(), "{relation} was classified as a free download");
        }
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
        assert_eq!(safe_epub_file_name("Bad: / title", &Url::parse("https://example.com/download?id=1").unwrap()), "Bad_ _ title.epub");
    }

    #[test]
    fn opds_1_honors_feed_xml_base() {
        let feed = OPDS_1.replace("<feed xmlns=\"http://www.w3.org/2005/Atom\">", "<feed xmlns=\"http://www.w3.org/2005/Atom\" xml:base=\"https://cdn.example/books/\">");
        let catalog = parse_catalog(Url::parse("https://example.com/catalog/root.xml").unwrap(), None, feed.as_bytes()).unwrap();
        let OpdsAcquisition::Epub(acquisition) = &catalog.publications[0].acquisitions[0] else { panic!("expected EPUB") };
        assert_eq!(acquisition.url, "https://cdn.example/books/books/1.epub");
    }

    #[tokio::test]
    async fn fetch_search_and_import_cover_the_complete_backend_flow() {
        let epub = Arc::new(test_epub());
        let application = Router::new()
            .route("/catalog", get(move |headers: HeaderMap| async move {
                if headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()) != Some("Basic cmVhZGVyOnNlY3JldA==") {
                    return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
                }
                let body = r#"{"metadata":{"title":"Remote books"},"links":[{"rel":"search","href":"/search{?query}","type":"application/opds+json","templated":true}],"publications":[{"metadata":{"identifier":"remote-1","title":"Downloaded book","author":"Remote Author"},"links":[{"rel":"download","href":"/book.epub","type":"application/epub+zip"}]}]}"#;
                Response::builder().header(header::CONTENT_TYPE, OPDS_2_MEDIA_TYPE).body(Body::from(body)).unwrap()
            }))
            .route("/search", get(|| async { Response::builder().header(header::CONTENT_TYPE, OPDS_2_MEDIA_TYPE).body(Body::from(r#"{"metadata":{"title":"Search results"},"navigation":[{"title":"No matches","href":"/catalog","type":"application/opds+json"}]}"#)).unwrap() }))
            .route("/book.epub", get(move |headers: HeaderMap| {
                let epub = epub.clone();
                async move {
                    if headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()) != Some("Basic cmVhZGVyOnNlY3JldA==") {
                        return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
                    }
                    Response::builder().header(header::CONTENT_TYPE, EPUB_MEDIA_TYPE).body(Body::from(epub.as_ref().clone())).unwrap()
                }
            }))
            .route("/bad.epub", get(|| async { Response::builder().header(header::CONTENT_TYPE, EPUB_MEDIA_TYPE).body(Body::from("not an epub")).unwrap() }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });

        let root = tempfile::tempdir().unwrap();
        let core = AppCore::initialize(AppDataLocation::PlatformLocal(root.path().join("app-data"))).unwrap();
        let library = core.create_library("Test".to_owned(), Some(root.path().join("library"))).unwrap();
        core.select_library(library.library_id().as_str()).unwrap();
        let credentials = OpdsCredentials::for_url(&format!("http://{address}/catalog"), "reader", "secret").unwrap();
        let catalog = core.opds_catalog(&format!("http://{address}/catalog"), Some(&credentials)).await.unwrap();
        let search = core.search_opds_catalog(&catalog, "a & b", None).await.unwrap();
        assert_eq!(search.title, "Search results");
        let OpdsAcquisition::Epub(acquisition) = &catalog.publications[0].acquisitions[0] else { panic!("expected EPUB") };
        let imported = core.import_opds_epub("__root__", acquisition, Some(&credentials)).await.unwrap();
        assert_eq!(imported.title().as_str(), "Downloaded book");
        assert_eq!(core.directory("__root__").unwrap().books.len(), 1);
        let temporary_directory = core.app_data_dir().join("opds-downloads");
        assert!(std::fs::read_dir(&temporary_directory).unwrap().next().is_none());
        let invalid = OpdsEpubAcquisition { url: format!("http://{address}/bad.epub"), kind: OpdsAcquisitionKind::OpenAccess, suggested_file_name: "bad.epub".to_owned() };
        assert!(core.import_opds_epub("__root__", &invalid, None).await.is_err());
        let paid = OpdsEpubAcquisition { url: format!("http://{address}/book.epub"), kind: OpdsAcquisitionKind::Buy, suggested_file_name: "paid.epub".to_owned() };
        assert!(core.import_opds_epub("__root__", &paid, Some(&credentials)).await.is_err());
        assert_eq!(core.directory("__root__").unwrap().books.len(), 1);
        assert!(std::fs::read_dir(&temporary_directory).unwrap().next().is_none());
        server.abort();
    }

    fn test_epub() -> Vec<u8> {
        let output = Cursor::new(Vec::new());
        let mut archive = zip::ZipWriter::new(output);
        let options = SimpleFileOptions::default();
        archive.start_file("mimetype", options.compression_method(zip::CompressionMethod::Stored)).unwrap();
        archive.write_all(b"application/epub+zip").unwrap();
        archive.start_file("META-INF/container.xml", options).unwrap();
        archive
            .write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)
            .unwrap();
        archive.start_file("OEBPS/content.opf", options).unwrap();
        archive.write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">remote-1</dc:identifier><dc:title>Downloaded book</dc:title><dc:creator>Remote Author</dc:creator></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#).unwrap();
        archive.start_file("OEBPS/chapter.xhtml", options).unwrap();
        archive.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body>Book</body></html>"#).unwrap();
        archive.finish().unwrap().into_inner()
    }
}
