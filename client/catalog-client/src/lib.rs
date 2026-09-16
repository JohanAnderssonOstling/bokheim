pub use catalog_core::CatalogCoverCache;
use catalog_core::{validate_server_cover, CatalogCoverError};
pub use catalog_model::{
    CatalogResultRange, CatalogResultRangeResponse, CatalogSubjectBrowseCapabilities, CatalogSubjectBrowseIndexResponse, CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse, CatalogSubjectPublicationPayloadRequest,
    CatalogSubjectPublicationPayloadResponse,
};

#[cfg(test)]
use catalog_model::CatalogPublication;
use catalog_model::{CatalogImportRequest, CatalogSourceId};
use futures_util::StreamExt;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogClientError(String);

impl CatalogClientError {
    pub fn new(message: impl std::fmt::Display) -> Self {
        Self(message.to_string())
    }
}

impl std::fmt::Display for CatalogClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CatalogClientError {}

impl From<rusqlite::Error> for CatalogClientError {
    fn from(error: rusqlite::Error) -> Self {
        Self::new(error)
    }
}

impl From<CatalogCoverError> for CatalogClientError {
    fn from(error: CatalogCoverError) -> Self {
        Self::new(error)
    }
}

const MAX_CATALOG_SERVER_RESPONSE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CATALOG_COVER_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CATALOG_ACQUISITION_BYTES: u64 = 1_000_000_000;
const CATALOG_ACQUISITION_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub struct CatalogClient {
    http: reqwest::Client,
    base_url: Url,
    store: client_store::ClientCatalogStore,
    covers: CatalogCoverCache,
}

impl CatalogClient {
    pub fn with_persistent_cache(server_base_url: &str, data_directory: &Path) -> Result<Self, CatalogClientError> {
        let base_url = Url::parse(server_base_url).map_err(CatalogClientError::new)?;
        if !matches!(base_url.scheme(), "http" | "https") || !base_url.username().is_empty() || base_url.password().is_some() || base_url.query().is_some() || base_url.fragment().is_some() {
            return Err(CatalogClientError::new("catalog server URL must be an HTTP origin or base path without credentials, query, or fragment"));
        }
        let cache_path = client_store::cache_path(data_directory);
        let store = client_store::ClientCatalogStore::open(&cache_path)?;
        let covers = CatalogCoverCache::new(data_directory.join("catalog-covers"))?;
        Ok(Self { http: reqwest::Client::new(), base_url, store, covers })
    }
}

mod client_store;
mod subject_payload_cache;

impl CatalogClient {
    fn endpoint(&self, path: &str) -> Result<Url, CatalogClientError> {
        self.base_url.join(path).map_err(CatalogClientError::new)
    }

    async fn post_wire<T: Serialize + ?Sized, R: serde::de::DeserializeOwned>(&self, path: &str, value: &T) -> Result<R, CatalogClientError> {
        let url = self.endpoint(path)?;
        let bytes = wire::encode(value).map_err(CatalogClientError::new)?;
        let response = tokio::time::timeout(Duration::from_secs(15), self.http.post(url).header(reqwest::header::ACCEPT, wire::MEDIA_TYPE).header(reqwest::header::CONTENT_TYPE, wire::MEDIA_TYPE).body(bytes).send())
            .await
            .map_err(|_| CatalogClientError::new("catalog server request timed out"))?
            .map_err(CatalogClientError::new)?;
        if !response.status().is_success() {
            let status = response.status();
            let detail = if response.content_length().is_some_and(|length| length <= 4096) {
                read_response_body(response, 4096, "error response").await.ok().and_then(|bytes| String::from_utf8(bytes).ok()).filter(|value| !value.trim().is_empty())
            } else {
                None
            };
            return Err(CatalogClientError::new(detail.map_or_else(|| format!("catalog server returned HTTP {status}"), |detail| format!("catalog server returned HTTP {status}: {detail}"))));
        }
        let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok());
        if !media_type.is_some_and(wire::is_current_media_type) {
            return Err(CatalogClientError::new("catalog server returned an unsupported media type"));
        }
        if response.headers().get(reqwest::header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()).is_some_and(|value| !value.eq_ignore_ascii_case("identity")) {
            return Err(CatalogClientError::new("catalog server returned a compressed response"));
        }
        let bytes = read_response_body(response, MAX_CATALOG_SERVER_RESPONSE_BYTES, "response").await?;
        wire::decode(&bytes, wire::MAX_DECODED_RESPONSE_BYTES).map_err(CatalogClientError::new)
    }
}

async fn read_response_body(response: reqwest::Response, max_bytes: u64, kind: &str) -> Result<Vec<u8>, CatalogClientError> {
    let too_large = || CatalogClientError::new(format!("catalog server {kind} is too large"));
    if response.content_length().is_some_and(|length| length > max_bytes) {
        return Err(too_large());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(CatalogClientError::new)?;
        if (bytes.len() as u64).saturating_add(chunk.len() as u64) > max_bytes {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(CatalogClientError::new(format!("catalog server {kind} is empty")));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Response, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use catalog_model::{CatalogPublication, CatalogPublicationIdentity};
    use image::{DynamicImage, ImageOutputFormat};
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn startup_uses_current_cache_without_modifying_legacy_schema() {
        let root = tempfile::tempdir().unwrap();
        let legacy_path = root.path().join("catalog-cache.db");
        let legacy = rusqlite::Connection::open(&legacy_path).unwrap();
        legacy.execute_batch(
            "CREATE TABLE catalog_client_schema_metadata(singleton INTEGER PRIMARY KEY,schema_version INTEGER NOT NULL);
             INSERT INTO catalog_client_schema_metadata VALUES(1,3);
             CREATE TABLE catalog_client_registry(singleton INTEGER PRIMARY KEY,payload BLOB NOT NULL);
             INSERT INTO catalog_client_registry VALUES(1,x'0304');"
        ).unwrap();
        drop(legacy);
        let original = std::fs::read(&legacy_path).unwrap();
        let client = CatalogClient::with_persistent_cache("https://example.com/", root.path()).unwrap();
        client.store.connection.lock().unwrap().execute("INSERT INTO catalog_subject_publication_cache VALUES('current',7,x'0506')", []).unwrap();
        drop(client);
        let reopened = CatalogClient::with_persistent_cache("https://example.com/", root.path()).unwrap();
        let payload: Vec<u8> = reopened.store.connection.lock().unwrap().query_row("SELECT publication_payload FROM catalog_subject_publication_cache WHERE entry_id=7", [], |row| row.get(0)).unwrap();
        assert_eq!(payload, [5,6]);
        assert_eq!(std::fs::read(legacy_path).unwrap(), original);
    }

    fn cover_response(source_id: &CatalogSourceId, image_urls: &[&str]) -> CatalogSubjectBrowseResponse {
        CatalogSubjectBrowseResponse {
            epoch: "epoch".to_owned(),
            navigation: Vec::new(),
            publications: image_urls
                .iter()
                .enumerate()
                .map(|(index, image_url)| catalog_model::CatalogSourcedPublication {
                    source_id: source_id.clone(),
                    publication: CatalogPublication {
                        identity: CatalogPublicationIdentity { edition_id: index.to_string(), work_id: None },
                        title: "Book".to_owned(),
                        authors: Vec::new(),
                        languages: Vec::new(),
                        description: None,
                        image_url: Some((*image_url).to_owned()),
                        metadata: Default::default(),
                        actions: Vec::new(),
                    },
                })
                .collect(),
            range: CatalogResultRangeResponse { offset: 0, returned_items: image_urls.len().try_into().unwrap(), has_more: false },
            capabilities: Default::default(),
        }
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
        let response = cover_response(&source_id, &[&image_url, &broken_url]);
        let cache = CatalogCoverCache::new(temporary.path().join("covers")).unwrap();
        assert!(cache.cover(&source_id, &image_url).await.is_err());
        cache.remember_subject_browse(&response);
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
        authorized_after_restart.remember_subject_browse(&response);
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
        let response = cover_response(&source_id, &[image_url]);
        let server_url = format!("http://{address}/");
        let client = CatalogClient::with_persistent_cache(&server_url, temporary.path()).unwrap();
        client.covers.remember_subject_browse(&response);
        let first = client.cover(&source_id, image_url).await.unwrap();
        assert!(first.starts_with(&[0xff, 0xd8]));
        assert_eq!(client.cover(&source_id, image_url).await.unwrap(), first);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        server_task.abort();
        drop(client);
        let restored = CatalogClient::with_persistent_cache(&server_url, temporary.path()).unwrap();
        assert_eq!(restored.cover(&source_id, image_url).await.unwrap(), first);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }
}
mod assets;
mod subjects;
