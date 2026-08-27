use catalog_core::validate_server_cover;
pub use catalog_core::{CatalogCache, CatalogCoverCache, CatalogLanguageFilter, CatalogProviderError, CatalogSearchMatches};
pub use catalog_model::{
    CatalogResultRange, CatalogResultRangeResponse, CatalogSubjectBrowseCapabilities, CatalogSubjectBrowseIndexResponse, CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse, CatalogSubjectPublicationPayloadRequest,
    CatalogSubjectPublicationPayloadResponse,
};
pub use catalog_protocol::{
    CatalogGroupLayout, CatalogPageLayout, CatalogPageRequest, CatalogPageResponse, CatalogPublicationPayload, CatalogPublicationReference, CatalogSearchRequest, CatalogSearchResponse, CatalogSourceRegistration, CatalogSourceRegistry,
};

use catalog_model::{CatalogFreshness, CatalogGroup, CatalogImportRequest, CatalogPage, CatalogPagination, CatalogSearch, CatalogSearchQuery, CatalogSource, CatalogSourceId};
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
    store: client_store::ClientCatalogStore,
    covers: CatalogCoverCache,
    registry: Mutex<registry::ServerRegistryStore>,
    languages: RwLock<CatalogLanguageFilter>,
}

pub struct CatalogDownload {
    bytes: Vec<u8>,
    file_name: String,
}

impl CatalogDownload {
    pub fn open(&self) -> Result<Box<dyn std::io::Read + '_>, CatalogProviderError> {
        Ok(Box::new(std::io::Cursor::new(self.bytes.as_slice())))
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
        let cache_path = data_directory.join("catalog-cache.db");
        let cache = CatalogCache::open(&cache_path).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let store = client_store::ClientCatalogStore::open(&cache_path)?;
        let covers = CatalogCoverCache::new(data_directory.join("catalog-covers"))?;
        let registry = registry::ServerRegistryStore::load(&store)?;
        Ok(Self { server, cache, store, covers, registry: Mutex::new(registry), languages: RwLock::new(CatalogLanguageFilter::new(languages)) })
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
    }

    fn catalog_sources(&self) -> Vec<CatalogSource> {
        self.registry.lock().map(|registry| registry.sources()).unwrap_or_default()
    }
}

mod client_store;
mod page_search_cache;
mod registry;
mod subject_payload_cache;

impl CatalogServerClient {
    pub fn new(base_url: &str) -> Result<Self, CatalogProviderError> {
        let base_url = Url::parse(base_url).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if !matches!(base_url.scheme(), "http" | "https") || !base_url.username().is_empty() || base_url.password().is_some() || base_url.query().is_some() || base_url.fragment().is_some() {
            return Err(CatalogProviderError::new("catalog server URL must be an HTTP origin or base path without credentials, query, or fragment"));
        }
        let http = reqwest::Client::new();
        Ok(Self { http, base_url })
    }

    fn endpoint(&self, path: &str) -> Result<Url, CatalogProviderError> {
        self.base_url.join(path).map_err(|error| CatalogProviderError::new(error.to_string()))
    }

    async fn get_wire<T: serde::de::DeserializeOwned>(&self, url: Url) -> Result<T, CatalogProviderError> {
        let response = tokio::time::timeout(Duration::from_secs(15), self.http.get(url).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).send())
            .await
            .map_err(|_| CatalogProviderError::new("catalog server request timed out"))?
            .map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Self::response_wire(response).await
    }

    async fn post_wire<T: Serialize + ?Sized, R: serde::de::DeserializeOwned>(&self, url: Url, value: &T) -> Result<R, CatalogProviderError> {
        let bytes = wire::encode(value).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let response = tokio::time::timeout(Duration::from_secs(15), self.http.post(url).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).header(reqwest::header::CONTENT_TYPE, wire::MEDIA_TYPE).body(bytes).send())
            .await
            .map_err(|_| CatalogProviderError::new("catalog server request timed out"))?
            .map_err(|error| CatalogProviderError::new(error.to_string()))?;
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
    use catalog_model::{CatalogPublication, CatalogPublicationIdentity, LanguageTag};
    use image::{DynamicImage, ImageOutputFormat};
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn sources_are_empty_until_the_server_registry_is_cached() {
        let temporary = tempfile::tempdir().unwrap();
        let client = CatalogClient::with_supported_languages("http://127.0.0.1:9", temporary.path(), ["en-US"]).unwrap();

        assert!(client.sources().is_empty());
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
