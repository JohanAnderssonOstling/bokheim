pub use catalog_contract::{
    CatalogGroupLayout, CatalogPageLayout, CatalogPageRequest, CatalogPageResponse, CatalogPublicationPayload, CatalogPublicationReference, CatalogResultRange, CatalogResultRangeResponse, CatalogSearchRequest, CatalogSearchResponse,
    CatalogSourceRegistration, CatalogSourceRegistry, CatalogSubjectBrowseCapabilities, CatalogSubjectBrowseCapabilitiesResponse, CatalogSubjectBrowseDeltaResponse, CatalogSubjectBrowseRefreshRequest, CatalogSubjectBrowseRequest,
    CatalogSubjectBrowseResponse, CatalogSubjectCollectionManifestRequest, CatalogSubjectCollectionManifestResponse, CatalogSubjectPublicationPayloadRequest, CatalogSubjectPublicationPayloadResponse,
};
use catalog_core::validate_server_cover;
pub use catalog_core::{
    canonicalize_opds_catalog_locator, normalize_project_gutenberg_page, normalize_project_gutenberg_publication, normalize_project_gutenberg_publications, normalize_project_gutenberg_sourced_publication_descriptions,
    opds_catalog_source_id, opds_page, opds_search_definition, opds_search_query, opds_source, validate_opds_catalog_locator, CatalogCache, CatalogCoverCache, CatalogLanguageFilter, CatalogProviderError, CatalogSearchMatches,
};
#[cfg(test)]
use catalog_core::{opds_endpoint_id, opds_publication, opds_search, validated_opds_import};

#[cfg(test)]
use catalog_contract::{CatalogBookFormat, CatalogDescription, CatalogPublication, CatalogPublicationAction};
use catalog_contract::{CatalogFreshness, CatalogGroup, CatalogImportRequest, CatalogPage, CatalogPagination, CatalogSearch, CatalogSearchQuery, CatalogSource, CatalogSourceId};
#[cfg(test)]
use opds::{OpdsAcquisitionKind, OpdsBookFormat, OpdsCatalog, OpdsDirectAcquisition, OpdsEndpoint, OpdsEndpointId, OpdsSearch};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, RwLock};
use std::time::Duration;
use url::Url;

const MAX_CATALOG_SERVER_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CATALOG_COVER_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CATALOG_ACQUISITION_BYTES: u64 = 1_000_000_000;
const CATALOG_ACQUISITION_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
pub struct CatalogServerClient {
    http: reqwest::Client,
    base_url: Url,
}

pub struct CatalogClient {
    server: CatalogServerClient,
    cache: CatalogCache,
    covers: CatalogCoverCache,
    registry: Mutex<registry::ServerRegistryStore>,
    languages: RwLock<CatalogLanguageFilter>,
}

pub struct CatalogDownload {
    path: tempfile::TempPath,
    file_name: String,
}

impl CatalogDownload {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }
}

impl CatalogClient {
    pub fn with_persistent_cache(server_base_url: &str, data_directory: &Path) -> Result<Self, CatalogProviderError> {
        Self::with_supported_languages(server_base_url, data_directory, std::iter::empty::<&str>())
    }

    pub fn with_supported_languages(server_base_url: &str, data_directory: &Path, languages: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Self, CatalogProviderError> {
        let server = CatalogServerClient::new(server_base_url)?;
        let cache = CatalogCache::open(data_directory.join("catalog-cache.db")).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let gutenberg = CatalogSourceId::parse("opds-project-gutenberg").map_err(|error| CatalogProviderError::new(error.to_string()))?;
        cache.rewrite_source_publications(&gutenberg, normalize_project_gutenberg_publication).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let covers = CatalogCoverCache::new(data_directory.join("catalog-covers"))?;
        let registry = registry::ServerRegistryStore::load(data_directory.join("catalog-sources.json"));
        Ok(Self { server, cache, covers, registry: Mutex::new(registry), languages: RwLock::new(CatalogLanguageFilter::new(languages)) })
    }

    pub fn set_supported_languages(&self, languages: impl IntoIterator<Item = impl AsRef<str>>) -> Result<(), CatalogProviderError> {
        *self.languages.write().map_err(|_| CatalogProviderError::new("catalog language preference lock was poisoned"))? = CatalogLanguageFilter::new(languages);
        Ok(())
    }

    fn language_filter(&self) -> CatalogLanguageFilter {
        self.languages.read().map(|languages| languages.clone()).unwrap_or_else(|_| CatalogLanguageFilter::new(std::iter::empty::<&str>()))
    }

    fn prepare_page(&self, source: &CatalogSource, page: &mut CatalogPage) {
        self.language_filter().filter_page(source, page);
        if source.id.as_str() == "opds-project-gutenberg" {
            normalize_project_gutenberg_page(page);
        }
    }

    fn catalog_sources(&self) -> Vec<CatalogSource> {
        self.registry.lock().map(|registry| registry.sources()).unwrap_or_default()
    }
}

mod registry;

impl CatalogServerClient {
    pub fn new(base_url: &str) -> Result<Self, CatalogProviderError> {
        let base_url = Url::parse(base_url).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if !matches!(base_url.scheme(), "http" | "https") || !base_url.username().is_empty() || base_url.password().is_some() || base_url.query().is_some() || base_url.fragment().is_some() {
            return Err(CatalogProviderError::new("catalog server URL must be an HTTP origin or base path without credentials, query, or fragment"));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .user_agent(format!("Bokheim/{} catalog-client", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Ok(Self { http, base_url })
    }

    fn endpoint(&self, path: &str) -> Result<Url, CatalogProviderError> {
        self.base_url.join(path).map_err(|error| CatalogProviderError::new(error.to_string()))
    }

    async fn get_wire<T: serde::de::DeserializeOwned>(&self, url: Url) -> Result<T, CatalogProviderError> {
        let response = self.http.get(url).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).send().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Self::response_wire(response).await
    }

    async fn post_wire<T: Serialize + ?Sized, R: serde::de::DeserializeOwned>(&self, url: Url, value: &T) -> Result<R, CatalogProviderError> {
        let bytes = wire::encode(value).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let response = self.http.post(url).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).header(reqwest::header::CONTENT_TYPE, wire::MEDIA_TYPE).body(bytes).send().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Self::response_wire(response).await
    }

    async fn response_wire<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T, CatalogProviderError> {
        if !response.status().is_success() {
            let status = response.status();
            let detail = if response.content_length().is_some_and(|length| length <= 4096) { response.bytes().await.ok().and_then(|bytes| String::from_utf8(bytes.to_vec()).ok()).filter(|value| !value.trim().is_empty()) } else { None };
            return Err(CatalogProviderError::new(detail.map_or_else(|| format!("catalog server returned HTTP {status}"), |detail| format!("catalog server returned HTTP {status}: {detail}"))));
        }
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
        if !media_type.is_some_and(wire::is_current_media_type) {
            return Err(CatalogProviderError::new("catalog server returned an unsupported media type"));
        }
        if response.headers().get(reqwest::header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()).is_some_and(|value| !value.eq_ignore_ascii_case("identity")) {
            return Err(CatalogProviderError::new("catalog server returned a compressed response"));
        }
        if response.content_length().is_some_and(|length| length > MAX_CATALOG_SERVER_RESPONSE_BYTES) {
            return Err(CatalogProviderError::new("catalog server response is too large"));
        }
        let bytes = response.bytes().await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
        wire::decode(&bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(|error| CatalogProviderError::new(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Response, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use catalog_contract::{CatalogExternalActionKind, CatalogLinkTarget, CatalogPublication, CatalogPublicationIdentity, CatalogSearchDefinition, CatalogSearchField, LanguageTag};
    use image::{DynamicImage, ImageOutputFormat};
    use opds::{OpdsAcquisition, OpdsAuthorCredit, OpdsAuthorLink, OpdsCatalogLink, OpdsFacet, OpdsIndirectAcquisition, OpdsLinkTarget, OpdsPagination, OpdsPublication, OpdsUnsupportedAcquisition, OpdsVersion};
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn sources_are_empty_until_the_server_registry_is_cached() {
        let temporary = tempfile::tempdir().unwrap();
        let client = CatalogClient::with_supported_languages("http://127.0.0.1:9", temporary.path(), ["en-US"]).unwrap();

        assert!(client.sources().is_empty());
    }

    #[test]
    fn cached_gutenberg_descriptions_drop_provider_boilerplate() {
        let mut publication = CatalogPublication {
            identity: CatalogPublicationIdentity { edition_id: "urn:gutenberg:2465:3".to_owned(), work_id: Some("urn:gutenberg:2465".to_owned()) },
            title: "Carmen".to_owned(),
            authors: Vec::new(),
            languages: vec![LanguageTag::parse("en").unwrap()],
            description: Some(CatalogDescription::RichHtml {
                safe_html: r#"<p>This edition has images.</p><p>Title: Carmen</p><p>Note: Wikipedia page about this book: <a href="https://en.wikipedia.org/wiki/Carmen_(novella)">Carmen</a></p><p>Summary: Useful text</p>"#.to_owned(),
                plain_text: "stale cached text".to_owned(),
            }),
            image_url: None,
            metadata: Default::default(),
            actions: Vec::new(),
        };

        normalize_project_gutenberg_publication(&mut publication);

        assert_eq!(publication.description, Some(CatalogDescription::RichHtml { safe_html: "<p>Useful text</p>".to_owned(), plain_text: "Useful text".to_owned() }));
    }

    #[test]
    fn gutenberg_work_duplicates_prefer_the_illustrated_edition() {
        let source_id = CatalogSourceId::parse("opds-project-gutenberg").unwrap();
        let edition = |edition_id: &str, download_url: &str| {
            let identity = CatalogPublicationIdentity { edition_id: edition_id.to_owned(), work_id: Some("urn:gutenberg:2465".to_owned()) };
            CatalogPublication {
                identity: identity.clone(),
                title: "Carmen".to_owned(),
                authors: Vec::new(),
                languages: vec![LanguageTag::parse("en").unwrap()],
                description: None,
                image_url: None,
                metadata: Default::default(),
                actions: vec![CatalogPublicationAction::Import {
                    request: CatalogImportRequest {
                        source_id: source_id.clone(),
                        catalog_locator: "https://www.gutenberg.org/ebooks/2465.opds".to_owned(),
                        publication: identity,
                        download_url: download_url.to_owned(),
                        book_format: CatalogBookFormat::Epub,
                    },
                }],
            }
        };
        let mut publications = vec![edition("urn:gutenberg:2465:2", "https://www.gutenberg.org/ebooks/2465.epub.noimages"), edition("urn:gutenberg:2465:3", "https://www.gutenberg.org/ebooks/2465.epub.images")];

        normalize_project_gutenberg_publications(&mut publications);

        assert_eq!(publications.len(), 1);
        assert_eq!(publications[0].identity.edition_id, "urn:gutenberg:2465:3");
    }

    #[test]
    fn opds_actions_preserve_all_import_formats_and_external_fulfillment() {
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let raw_publication = OpdsPublication {
            id: "edition".to_owned(),
            work_id: Some("work".to_owned()),
            title: "Book".to_owned(),
            authors: vec![OpdsAuthorCredit {
                name: "Author".to_owned(),
                name_localizations: Vec::new(),
                sort_name: Some("Author, An".to_owned()),
                sort_name_localizations: Vec::new(),
                identifiers: vec!["https://isni.org/isni/0000000121400562".to_owned()],
                provider_link: Some(OpdsAuthorLink { url: "https://catalog.example/authors/1".to_owned(), target: OpdsLinkTarget::Catalog }),
            }],
            languages: vec!["en".to_owned()],
            description: None,
            image_url: None,
            metadata: Default::default(),
            preferred_download: None,
            acquisitions: vec![
                OpdsAcquisition::Direct(OpdsDirectAcquisition {
                    url: "https://downloads.example/book.epub".to_owned(),
                    kind: OpdsAcquisitionKind::OpenAccess,
                    book_format: OpdsBookFormat::Epub,
                    suggested_file_name: "book.epub".to_owned(),
                    price: None,
                }),
                OpdsAcquisition::Direct(OpdsDirectAcquisition {
                    url: "https://downloads.example/book-legacy.epub".to_owned(),
                    kind: OpdsAcquisitionKind::OpenAccess,
                    book_format: OpdsBookFormat::Epub,
                    suggested_file_name: "book-legacy.epub".to_owned(),
                    price: None,
                }),
                OpdsAcquisition::Direct(OpdsDirectAcquisition {
                    url: "https://example.com/book.pdf".to_owned(),
                    kind: OpdsAcquisitionKind::OpenAccess,
                    book_format: OpdsBookFormat::Pdf,
                    suggested_file_name: "book.pdf".to_owned(),
                    price: None,
                }),
                OpdsAcquisition::Unsupported(OpdsUnsupportedAcquisition {
                    url: "https://downloads.example/book.kindle".to_owned(),
                    kind: OpdsAcquisitionKind::Generic,
                    media_type: Some("application/x-mobipocket-ebook".to_owned()),
                    price: None,
                }),
                OpdsAcquisition::Indirect(OpdsIndirectAcquisition {
                    url: "https://example.com/borrow".to_owned(),
                    kind: OpdsAcquisitionKind::Borrow,
                    media_type: Some("text/html".to_owned()),
                    available_formats: vec![OpdsBookFormat::Epub],
                    price: None,
                }),
            ],
        };
        let catalog = OpdsCatalog {
            version: OpdsVersion::V2_0,
            freshness: opds::OpdsCatalogFreshness::Current,
            title: "Catalog".to_owned(),
            source_url: "https://example.com/catalog".to_owned(),
            self_url: None,
            start_url: None,
            navigation: Vec::new(),
            publications: vec![raw_publication.clone()],
            groups: Vec::new(),
            facets: Vec::new(),
            pagination: OpdsPagination::default(),
            search: None,
        };
        let publication = opds_publication(&source_id, &catalog.source_url, raw_publication);
        assert_eq!(publication.authors[0].name, "Author");
        assert_eq!(publication.authors[0].sort_name.as_deref(), Some("Author, An"));
        assert_eq!(publication.authors[0].identifiers, vec!["https://isni.org/isni/0000000121400562"]);
        assert_eq!(publication.authors[0].provider_link, Some(CatalogLinkTarget::Page { locator: "https://catalog.example/authors/1".to_owned() }));
        assert_eq!(publication.actions.len(), 4);
        assert!(matches!(publication.actions[0], CatalogPublicationAction::Import { ref request } if request.book_format == CatalogBookFormat::Epub));
        assert!(matches!(publication.actions[1], CatalogPublicationAction::Import { ref request } if request.book_format == CatalogBookFormat::Pdf));
        assert!(matches!(publication.actions[2], CatalogPublicationAction::External { purpose: CatalogExternalActionKind::Open, .. }));
        assert!(matches!(publication.actions[3], CatalogPublicationAction::External { purpose: CatalogExternalActionKind::Borrow, .. }));
        let CatalogPublicationAction::Import { request } = &publication.actions[0] else { panic!("expected import action") };
        assert_eq!(request.catalog_locator, catalog.source_url);
        let (identity, acquisition) = validated_opds_import(request, &catalog).unwrap();
        assert_eq!(identity.publication_id, "edition");
        assert_eq!(acquisition.url, "https://downloads.example/book.epub");
        assert_eq!(acquisition.suggested_file_name, "book.epub");
        let mut forged = request.clone();
        forged.download_url = "https://attacker.example/forged.epub".to_owned();
        assert!(validated_opds_import(&forged, &catalog).is_err());
    }

    #[test]
    fn catalog_facets_hide_provider_metadata_and_compact_supported_formats() {
        let link = |title: &str| OpdsCatalogLink { title: title.to_owned(), url: format!("https://example.com/{}", title.replace(' ', "-")), target: OpdsLinkTarget::Catalog, relations: Vec::new(), number_of_items: None, active: false };
        let catalog = OpdsCatalog {
            version: OpdsVersion::V1_2,
            freshness: opds::OpdsCatalogFreshness::Current,
            title: "Catalog".to_owned(),
            source_url: "https://example.com/catalog".to_owned(),
            self_url: None,
            start_url: None,
            navigation: Vec::new(),
            publications: Vec::new(),
            groups: Vec::new(),
            facets: vec![
                OpdsFacet { title: "Order".to_owned(), links: vec![link("Newest")] },
                OpdsFacet { title: "Format".to_owned(), links: vec![link("eBooks available in format: pdf"), link("eBooks available in format: epub"), link("MOBI")] },
                OpdsFacet { title: "License".to_owned(), links: vec![link("Public domain")] },
                OpdsFacet { title: "Collection".to_owned(), links: vec![link("indexed in DOAB")] },
            ],
            pagination: OpdsPagination::default(),
            search: None,
        };
        let page = opds_page(CatalogSourceId::parse("opds-test").unwrap(), catalog);
        assert_eq!(page.facets.iter().map(|facet| facet.title.as_str()).collect::<Vec<_>>(), vec!["Order", "Format"]);
        assert_eq!(page.facets[1].links.iter().map(|link| link.title.as_str()).collect::<Vec<_>>(), vec!["PDF", "EPUB", "MOBI/AZW"]);
    }

    #[test]
    fn provider_routing_validates_source_namespace_and_search_definitions() {
        assert_eq!(opds_endpoint_id(&CatalogSourceId::parse("opds-example").unwrap()).unwrap().as_str(), "example");
        assert!(opds_endpoint_id(&CatalogSourceId::parse("unknown").unwrap()).is_err());
        let endpoint = OpdsEndpoint { id: OpdsEndpointId::parse("example").unwrap(), title: "Example".to_owned(), description: "Example".to_owned(), languages: vec!["English".to_owned()], url: "https://catalog.example/root".to_owned() };
        assert!(validate_opds_catalog_locator(&endpoint, "https://catalog.example/page").is_ok());
        assert_eq!(canonicalize_opds_catalog_locator(&endpoint, "http://catalog.example/page").unwrap(), "https://catalog.example/page");
        assert!(validate_opds_catalog_locator(&endpoint, "https://attacker.example/page").is_err());
        assert!(validate_opds_catalog_locator(&endpoint, "http://catalog.example:8080/page").is_err());
        assert!(validate_opds_catalog_locator(&endpoint, "https://user:secret@catalog.example/page").is_err());
    }

    #[test]
    fn opds_search_only_advertises_fields_supported_by_its_template() {
        let simple = opds_search(OpdsSearch::UriTemplate { base_url: "https://example.com".to_owned(), template: "search{?searchTerms}".to_owned() });
        assert_eq!(simple.fields, vec![CatalogSearchField::Query]);
        assert!(matches!(&simple.definition, CatalogSearchDefinition::OpdsUriTemplate { base_url, template } if base_url == "https://example.com" && template == "search{?searchTerms}"));
        let advanced = opds_search(OpdsSearch::UriTemplate { base_url: "https://example.com".to_owned(), template: "search{?query,title,author}".to_owned() });
        assert_eq!(advanced.fields, vec![CatalogSearchField::Query, CatalogSearchField::Title, CatalogSearchField::Author]);
        let encoded = wire::encode(&advanced).unwrap();
        assert_eq!(wire::decode::<CatalogSearch>(&encoded, wire::MAX_DECODED_RESPONSE_BYTES).unwrap(), advanced);
    }

    #[tokio::test]
    async fn catalog_covers_are_authorized_normalized_and_reused_from_disk() {
        let requests = Arc::new(AtomicUsize::new(0));
        let request_counter = requests.clone();
        let failures = Arc::new(AtomicUsize::new(0));
        let failure_counter = failures.clone();
        let mut png = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(400, 600).write_to(&mut png, ImageOutputFormat::Png).unwrap();
        let source_bytes = png.into_inner();
        let application = Router::new()
            .route(
                "/cover.png",
                get(move || {
                    request_counter.fetch_add(1, Ordering::SeqCst);
                    let bytes = source_bytes.clone();
                    async move { Response::builder().header(header::CONTENT_TYPE, "image/png").body(Body::from(bytes)).unwrap() }
                }),
            )
            .route(
                "/broken.png",
                get(move || {
                    failure_counter.fetch_add(1, Ordering::SeqCst);
                    async { StatusCode::BAD_GATEWAY }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let image_url = format!("http://{address}/cover.png");
        let broken_url = format!("http://{address}/broken.png");
        let page = CatalogPage {
            source_id: source_id.clone(),
            freshness: CatalogFreshness::Current,
            title: "Covers".to_owned(),
            locator: format!("http://{address}/root"),
            navigation: Vec::new(),
            publications: vec![
                CatalogPublication {
                    identity: CatalogPublicationIdentity { edition_id: "edition".to_owned(), work_id: None },
                    title: "Book".to_owned(),
                    authors: Vec::new(),
                    languages: Vec::new(),
                    description: None,
                    image_url: Some(image_url.clone()),
                    metadata: Default::default(),
                    actions: Vec::new(),
                },
                CatalogPublication {
                    identity: CatalogPublicationIdentity { edition_id: "broken".to_owned(), work_id: None },
                    title: "Broken".to_owned(),
                    authors: Vec::new(),
                    languages: Vec::new(),
                    description: None,
                    image_url: Some(broken_url.clone()),
                    metadata: Default::default(),
                    actions: Vec::new(),
                },
            ],
            groups: Vec::new(),
            facets: Vec::new(),
            pagination: CatalogPagination::default(),
            search: None,
        };
        let cache = CatalogCoverCache::new(temporary.path().join("covers")).unwrap();
        assert!(cache.cover(&source_id, &image_url).await.is_err());
        cache.remember_page(&page);
        let first = cache.cover(&source_id, &image_url).await.unwrap();
        assert!(first.starts_with(&[0xff, 0xd8]));
        assert_eq!(cache.cover(&source_id, &image_url).await.unwrap(), first);
        let restored = CatalogCoverCache::new(temporary.path().join("covers")).unwrap();
        assert_eq!(restored.cover(&source_id, &image_url).await.unwrap(), first);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert!(cache.cover(&source_id, &broken_url).await.is_err());
        assert!(cache.cover(&source_id, &broken_url).await.is_err());
        assert_eq!(failures.load(Ordering::SeqCst), 2);
        let cached_cover = std::fs::read_dir(temporary.path().join("covers")).unwrap().filter_map(Result::ok).map(|entry| entry.path()).find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("jpg")).unwrap();
        std::fs::remove_file(cached_cover).unwrap();
        let authorized_after_restart = CatalogCoverCache::new(temporary.path().join("covers")).unwrap();
        assert!(authorized_after_restart.cover(&source_id, &image_url).await.is_err());
        authorized_after_restart.remember_page(&page);
        assert_eq!(authorized_after_restart.cover(&source_id, &image_url).await.unwrap(), first);
        assert_eq!(requests.load(Ordering::SeqCst), 2);
        server.abort();
    }

    #[tokio::test]
    async fn local_cover_cache_fetches_server_covers_only_once() {
        const IMAGE_URL: &str = "/api/opds/covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.jpg";
        let requests = Arc::new(AtomicUsize::new(0));
        let request_counter = requests.clone();
        let mut jpeg = Cursor::new(Vec::new());
        // Embedded EPUB thumbnails use the library's 600 px cover contract,
        // which is intentionally larger than URL covers normalized by the
        // catalogue cache.
        DynamicImage::new_rgb8(600, 960).write_to(&mut jpeg, ImageOutputFormat::Jpeg(85)).unwrap();
        let server_bytes = jpeg.into_inner();
        let application = Router::new().route(
            IMAGE_URL,
            get(move || {
                request_counter.fetch_add(1, Ordering::SeqCst);
                let bytes = server_bytes.clone();
                async move { Response::builder().header(header::CONTENT_TYPE, "image/jpeg").body(Body::from(bytes)).unwrap() }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move { axum::serve(listener, application).await.unwrap() });
        let temporary = tempfile::tempdir().unwrap();
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let image_url = IMAGE_URL;
        let page = CatalogPage {
            source_id: source_id.clone(),
            freshness: CatalogFreshness::Current,
            title: "Covers".to_owned(),
            locator: "https://catalog.example/root".to_owned(),
            navigation: Vec::new(),
            publications: vec![CatalogPublication {
                identity: CatalogPublicationIdentity { edition_id: "edition".to_owned(), work_id: None },
                title: "Book".to_owned(),
                authors: Vec::new(),
                languages: Vec::new(),
                description: None,
                image_url: Some(image_url.to_owned()),
                metadata: Default::default(),
                actions: Vec::new(),
            }],
            groups: Vec::new(),
            facets: Vec::new(),
            pagination: CatalogPagination::default(),
            search: None,
        };
        let server = CatalogServerClient::new(&format!("http://{address}/")).unwrap();
        let cache_path = temporary.path().join("covers");
        let cache = CatalogCoverCache::new(cache_path.clone()).unwrap();
        cache.remember_page(&page);
        let first = cache.cover_with(&source_id, image_url, server.catalog_cover(&source_id, image_url)).await.unwrap();
        assert!(first.starts_with(&[0xff, 0xd8]));
        assert_eq!(cache.cover_with(&source_id, image_url, server.catalog_cover(&source_id, image_url)).await.unwrap(), first);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        server_task.abort();
        let restored = CatalogCoverCache::new(cache_path).unwrap();
        assert_eq!(restored.cover_with(&source_id, image_url, server.catalog_cover(&source_id, image_url)).await.unwrap(), first);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }
}
mod assets;
mod sources;
mod subjects;
