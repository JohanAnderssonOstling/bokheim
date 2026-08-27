mod cache;
#[cfg(feature = "provider")]
mod language;

#[cfg(feature = "server-extension")]
#[doc(hidden)]
pub use cache::server_extension;
pub use cache::{
    active_generation_in, ensure_source_in, prepare_catalog_subject_assignments, put_page_row, refresh_active_counts, sequence_in, sql_decode_error, storage_i64, stored_u64, upsert_publication_with_sequence, CachedCatalogPage,
    CatalogCache, CatalogCacheError, CatalogSearchMatches, CatalogSourceState, CatalogSubjectAssignment, SequencedCatalogPublication,
};
#[cfg(feature = "provider")]
pub use language::CatalogLanguageFilter;

#[cfg(feature = "provider")]
mod provider {
    use catalog_model::{
        CatalogAuthentication, CatalogAuthorCredit, CatalogBookFormat, CatalogCapability, CatalogContributorCredit, CatalogExternalActionKind, CatalogFacet, CatalogFreshness, CatalogGroup, CatalogImportRequest, CatalogLink,
        CatalogLinkTarget, CatalogPage, CatalogPagination, CatalogPrice, CatalogPublicationIdentity, CatalogPublicationMetadata, CatalogPublisherCredit, CatalogSearch, CatalogSearchDefinition, CatalogSearchField, CatalogSearchQuery,
        CatalogSource, CatalogSourceId, CatalogSubjectBrowseResponse, Identifier, LanguageTag, LocalizedText, Scheme, Scope,
    };
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    use futures_util::StreamExt;
    #[cfg(feature = "cover-cache")]
    use image::io::Reader as ImageReader;
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::fmt;
    use std::future::Future;
    #[cfg(feature = "cover-cache")]
    use std::io::Cursor;
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    use std::path::Path;
    #[cfg(feature = "cover-cache")]
    use std::path::PathBuf;
    #[cfg(feature = "cover-cache")]
    use std::sync::{Arc, Mutex};
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    use std::time::Duration;
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    use tokio::io::AsyncWriteExt;
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    use tokio::sync::mpsc;
    #[cfg(feature = "cover-cache")]
    use tokio::sync::{Mutex as AsyncMutex, Semaphore};
    #[cfg(not(target_arch = "wasm32"))]
    use url::Url;

    // Keep the queued compressed-image working set small. Catalogue crawls can
    // discover thousands of covers faster than the CPU can decode them, and each
    // queued body is retained until a resize worker receives it.
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    const MAX_CATALOG_COVER_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    const MAX_CATALOG_COVER_DECODE_WIDTH: u32 = 2048;
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    const MAX_CATALOG_COVER_DECODE_HEIGHT: u32 = 3072;
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    const MAX_CATALOG_COVER_DECODE_ALLOC: u64 = 28 * 1024 * 1024;
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    const CATALOG_COVER_WIDTH: u32 = 408;
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    const CATALOG_COVER_HEIGHT: u32 = 612;
    // Embedded EPUB thumbnails use the library thumbnail contract (600 px wide)
    // and may therefore be larger than covers normalized from upstream URLs.
    #[cfg(feature = "cover-cache")]
    const MAX_SERVER_COVER_WIDTH: u32 = 1200;
    #[cfg(feature = "cover-cache")]
    const MAX_SERVER_COVER_HEIGHT: u32 = 1920;
    #[cfg(feature = "cover-cache")]
    const MAX_CATALOG_COVER_URL_BYTES: usize = 4096;
    #[cfg(feature = "cover-cache")]
    const MAX_CONCURRENT_CATALOG_COVERS: usize = 4;
    #[cfg(feature = "cover-cache")]
    const MAX_REMEMBERED_COVER_REFERENCES: usize = 8_192;
    #[cfg(feature = "cover-cache")]
    const MAX_REMEMBERED_UNSUITABLE_COVERS: usize = 8_192;
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    const COVER_DOWNLOAD_QUEUE_CAPACITY: usize = 1_024;
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    const COVER_RESIZE_QUEUE_CAPACITY: usize = 4;
    #[cfg(feature = "cover-cache")]
    const CATALOG_COVER_ASSET_PATH_PREFIX: &str = "/api/opds/covers/";

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum CatalogProviderErrorKind {
        Other,
        UnsuitableCoverDimensions,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct CatalogProviderError {
        message: String,
        kind: CatalogProviderErrorKind,
    }

    impl CatalogProviderError {
        pub fn new(message: impl Into<String>) -> Self {
            Self { message: message.into(), kind: CatalogProviderErrorKind::Other }
        }

        #[cfg(not(target_arch = "wasm32"))]
        fn unsuitable_cover_dimensions(width: u32, height: u32) -> Self {
            Self { message: format!("catalog cover dimensions {width}x{height} are not suitable for a book cover"), kind: CatalogProviderErrorKind::UnsuitableCoverDimensions }
        }

        fn known_unsuitable_cover_dimensions() -> Self {
            Self { message: "catalog cover dimensions are not suitable for a book cover".to_owned(), kind: CatalogProviderErrorKind::UnsuitableCoverDimensions }
        }

        pub fn is_unsuitable_cover_dimensions(&self) -> bool {
            self.kind == CatalogProviderErrorKind::UnsuitableCoverDimensions
        }
    }

    impl fmt::Display for CatalogProviderError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(&self.message)
        }
    }

    impl std::error::Error for CatalogProviderError {}

    #[cfg(feature = "cover-cache")]
    #[derive(Clone)]
    pub struct CatalogCoverCache {
        #[cfg(not(target_arch = "wasm32"))]
        outbound_policy: outbound::OutboundNetworkPolicy,
        #[cfg(not(target_arch = "wasm32"))]
        directory: Arc<PathBuf>,
        known: Arc<Mutex<BoundedStringSet>>,
        unsuitable: Arc<Mutex<BoundedStringSet>>,
        in_flight: Arc<AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
        downloads: Arc<Semaphore>,
        #[cfg(target_arch = "wasm32")]
        cached: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    }

    #[derive(Clone)]
    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    pub struct CatalogCoverPipeline {
        cache: CatalogCoverCache,
        downloads: mpsc::Sender<CatalogCoverDownload>,
        scheduled: Arc<Mutex<HashSet<String>>>,
    }

    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    struct CatalogCoverDownload {
        source_id: CatalogSourceId,
        image_url: String,
    }

    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    struct CatalogCoverResize {
        image_url: String,
        source: Vec<u8>,
    }

    #[cfg(feature = "cover-cache")]
    struct BoundedStringSet {
        values: HashSet<String>,
        order: VecDeque<String>,
        capacity: usize,
    }

    #[cfg(feature = "cover-cache")]
    impl BoundedStringSet {
        fn new(capacity: usize) -> Self {
            Self { values: HashSet::new(), order: VecDeque::new(), capacity }
        }

        fn insert(&mut self, value: String) -> bool {
            if !self.values.insert(value.clone()) {
                return false;
            }
            self.order.push_back(value);
            while self.values.len() > self.capacity {
                if let Some(oldest) = self.order.pop_front() {
                    self.values.remove(&oldest);
                }
            }
            true
        }

        fn contains(&self, value: &str) -> bool {
            self.values.contains(value)
        }

        fn remove(&mut self, value: &str) -> bool {
            let removed = self.values.remove(value);
            if removed {
                self.order.retain(|queued| queued != value);
            }
            removed
        }
    }

    /// App-facing catalogue component. It composes the remote catalogue server
    /// with direct OPDS fallback, endpoint persistence, and cover caching.
    #[cfg(feature = "cover-cache")]
    impl CatalogCoverCache {
        pub fn new(directory: PathBuf) -> Result<Self, CatalogProviderError> {
            Self::build(directory)
        }

        #[cfg(not(target_arch = "wasm32"))]
        /// Creates the cover cache used by an Internet-facing service. Cover URLs
        /// and every redirect hop are restricted to publicly routed addresses.
        pub fn new_public_only(directory: PathBuf) -> Result<Self, CatalogProviderError> {
            Self::with_outbound_policy(directory, outbound::OutboundNetworkPolicy::PublicOnly)
        }

        #[cfg(not(target_arch = "wasm32"))]
        fn build(directory: PathBuf) -> Result<Self, CatalogProviderError> {
            Self::with_outbound_policy(directory, outbound::OutboundNetworkPolicy::AllowPrivate)
        }

        #[cfg(target_arch = "wasm32")]
        fn build(_directory: PathBuf) -> Result<Self, CatalogProviderError> {
            Ok(Self {
                known: Arc::new(Mutex::new(BoundedStringSet::new(MAX_REMEMBERED_COVER_REFERENCES))),
                unsuitable: Arc::new(Mutex::new(BoundedStringSet::new(MAX_REMEMBERED_UNSUITABLE_COVERS))),
                in_flight: Arc::new(AsyncMutex::new(HashMap::new())),
                downloads: Arc::new(Semaphore::new(MAX_CONCURRENT_CATALOG_COVERS)),
                cached: Arc::new(Mutex::new(HashMap::new())),
            })
        }

        #[cfg(not(target_arch = "wasm32"))]
        fn with_outbound_policy(directory: PathBuf, outbound_policy: outbound::OutboundNetworkPolicy) -> Result<Self, CatalogProviderError> {
            std::fs::create_dir_all(&directory).map_err(|error| CatalogProviderError::new(error.to_string()))?;
            let directory = directory.canonicalize().map_err(|error| CatalogProviderError::new(error.to_string()))?;
            Ok(Self {
                outbound_policy,
                directory: Arc::new(directory),
                known: Arc::new(Mutex::new(BoundedStringSet::new(MAX_REMEMBERED_COVER_REFERENCES))),
                unsuitable: Arc::new(Mutex::new(BoundedStringSet::new(MAX_REMEMBERED_UNSUITABLE_COVERS))),
                in_flight: Arc::new(AsyncMutex::new(HashMap::new())),
                downloads: Arc::new(Semaphore::new(MAX_CONCURRENT_CATALOG_COVERS)),
            })
        }

        pub fn remember_page(&self, page: &CatalogPage) {
            let references =
                page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).filter_map(|publication| publication.image_url.as_ref().map(|image_url| (page.source_id.clone(), image_url.clone()))).collect();
            self.remember_cover_urls(references);
        }

        pub fn remember_cover_url(&self, source_id: &CatalogSourceId, image_url: &str) {
            if image_url.len() <= MAX_CATALOG_COVER_URL_BYTES {
                self.remember_cover_urls(vec![(source_id.clone(), image_url.to_owned())]);
            }
        }

        pub fn remember_subject_browse(&self, response: &CatalogSubjectBrowseResponse) {
            let references = response.publications.iter().filter_map(|item| item.publication.image_url.as_ref().map(|image_url| (item.source_id.clone(), image_url.clone()))).collect();
            self.remember_cover_urls(references);
        }

        fn remember_cover_urls(&self, references: Vec<(CatalogSourceId, String)>) {
            let mut known = self.known.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            for (source_id, image_url) in references {
                if image_url.len() <= MAX_CATALOG_COVER_URL_BYTES {
                    known.insert(cover_reference_key(&source_id, &image_url));
                }
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub async fn cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
            self.cover_with(source_id, image_url, self.fetch_cover(image_url)).await
        }

        /// Reads an already generated JPEG without contacting the upstream OPDS
        /// host. The cache server uses this on its request path; missing covers are
        /// retried by the next crawl instead of delaying a user request.
        pub async fn cached_cover(&self, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
            if image_url.len() > MAX_CATALOG_COVER_URL_BYTES {
                return Err(CatalogProviderError::new("catalog cover URL is too long"));
            }
            if let Some(bytes) = self.read_cover(image_url).await {
                return Ok(bytes);
            }
            if self.unsuitable.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).contains(&cover_cache_key(image_url)) {
                return Err(CatalogProviderError::known_unsuitable_cover_dimensions());
            }
            Err(CatalogProviderError::new("catalog cover thumbnail is not cached"))
        }

        /// Promotes a normalized URL-keyed cache entry to an immutable,
        /// content-addressed asset and returns the public server path. Existing
        /// cache directories are migrated lazily as catalogue responses are
        /// served, so rollout does not require a separate backfill.
        pub async fn public_cover_reference(&self, image_url: &str) -> Option<String> {
            if let Some(digest) = catalog_cover_asset_digest(image_url) {
                return self.cached_cover_asset(digest).await.ok().map(|_| image_url.to_owned());
            }
            if image_url.len() > MAX_CATALOG_COVER_URL_BYTES {
                return None;
            }
            let bytes = self.read_cover(image_url).await?;
            let digest = blake3::hash(&bytes).to_hex().to_string();
            if self.read_cover_asset(&digest).await.is_none() && self.write_cover_asset(&digest, &bytes).await.is_err() {
                return None;
            }
            Some(catalog_cover_asset_reference(&digest))
        }

        /// Reads an immutable cover by its normalized JPEG content digest.
        pub async fn cached_cover_asset(&self, digest: &str) -> Result<Vec<u8>, CatalogProviderError> {
            if !valid_cover_asset_digest(digest) {
                return Err(CatalogProviderError::new("catalog cover asset digest is invalid"));
            }
            let bytes = self.read_cover_asset(digest).await.ok_or_else(|| CatalogProviderError::new("catalog cover asset is not cached"))?;
            if blake3::hash(&bytes).to_hex().as_str() != digest {
                return Err(CatalogProviderError::new("catalog cover asset does not match its digest"));
            }
            Ok(bytes)
        }

        #[doc(hidden)]
        pub async fn cover_with(&self, source_id: &CatalogSourceId, image_url: &str, fetch: impl Future<Output = Result<Vec<u8>, CatalogProviderError>>) -> Result<Vec<u8>, CatalogProviderError> {
            if image_url.len() > MAX_CATALOG_COVER_URL_BYTES {
                return Err(CatalogProviderError::new("catalog cover URL is too long"));
            }
            let reference_key = cover_reference_key(source_id, image_url);
            let cache_key = cover_cache_key(image_url);
            if let Some(bytes) = self.read_cover(image_url).await {
                return Ok(bytes);
            }
            if !self.known.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).contains(&reference_key) {
                return Err(CatalogProviderError::new("catalog cover URL was not present in a returned catalog page"));
            }
            let lock = {
                let mut in_flight = self.in_flight.lock().await;
                in_flight.entry(cache_key.clone()).or_insert_with(|| Arc::new(AsyncMutex::new(()))).clone()
            };
            let _fetch = lock.lock().await;
            if let Some(bytes) = self.read_cover(image_url).await {
                self.in_flight.lock().await.remove(&cache_key);
                return Ok(bytes);
            }
            let _download = match self.downloads.acquire().await {
                Ok(download) => download,
                Err(error) => {
                    self.in_flight.lock().await.remove(&cache_key);
                    return Err(CatalogProviderError::new(error.to_string()));
                }
            };
            let result = match fetch.await {
                Ok(bytes) => self.write_cover(image_url, &bytes).await.map(|()| bytes),
                Err(error) => Err(error),
            };
            {
                let mut unsuitable = self.unsuitable.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if result.as_ref().is_err_and(|error| error.is_unsuitable_cover_dimensions()) {
                    unsuitable.insert(cache_key.clone());
                } else if result.is_ok() {
                    unsuitable.remove(&cache_key);
                }
            }
            self.in_flight.lock().await.remove(&cache_key);
            result
        }

        #[cfg(not(target_arch = "wasm32"))]
        async fn read_cover(&self, image_url: &str) -> Option<Vec<u8>> {
            read_cached_cover(&cover_path(&self.directory, image_url)).await
        }

        #[cfg(target_arch = "wasm32")]
        async fn read_cover(&self, image_url: &str) -> Option<Vec<u8>> {
            self.cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&format!("url\n{image_url}")).cloned()
        }

        #[cfg(not(target_arch = "wasm32"))]
        async fn write_cover(&self, image_url: &str, bytes: &[u8]) -> Result<(), CatalogProviderError> {
            self.publish_cover(&cover_path(&self.directory, image_url), bytes).await
        }

        #[cfg(target_arch = "wasm32")]
        async fn write_cover(&self, image_url: &str, bytes: &[u8]) -> Result<(), CatalogProviderError> {
            self.cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(format!("url\n{image_url}"), bytes.to_vec());
            Ok(())
        }

        #[cfg(not(target_arch = "wasm32"))]
        async fn read_cover_asset(&self, digest: &str) -> Option<Vec<u8>> {
            read_cached_cover(&cover_asset_path(&self.directory, digest)).await
        }

        #[cfg(target_arch = "wasm32")]
        async fn read_cover_asset(&self, digest: &str) -> Option<Vec<u8>> {
            self.cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&format!("asset\n{digest}")).cloned()
        }

        #[cfg(not(target_arch = "wasm32"))]
        async fn write_cover_asset(&self, digest: &str, bytes: &[u8]) -> Result<(), CatalogProviderError> {
            self.publish_cover(&cover_asset_path(&self.directory, digest), bytes).await
        }

        #[cfg(target_arch = "wasm32")]
        async fn write_cover_asset(&self, digest: &str, bytes: &[u8]) -> Result<(), CatalogProviderError> {
            self.cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(format!("asset\n{digest}"), bytes.to_vec());
            Ok(())
        }

        #[cfg(not(target_arch = "wasm32"))]
        async fn fetch_cover(&self, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
            let source = self.fetch_cover_source(image_url).await?;
            tokio::task::spawn_blocking(move || normalize_cover(&source)).await.map_err(|error| CatalogProviderError::new(error.to_string()))?
        }

        #[cfg(not(target_arch = "wasm32"))]
        async fn fetch_cover_source(&self, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
            let url = Url::parse(image_url).map_err(|error| CatalogProviderError::new(error.to_string()))?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
                return Err(CatalogProviderError::new("catalog cover URL must be absolute credential-free HTTP(S)"));
            }
            let response = tokio::time::timeout(
                Duration::from_secs(15),
                outbound::send_get(url, reqwest::header::HeaderMap::new(), self.outbound_policy, &format!("Bokheim/{} catalog-cover-client", env!("CARGO_PKG_VERSION")), Duration::from_secs(5), Duration::from_secs(15)),
            )
            .await
            .map_err(|_| CatalogProviderError::new("catalog cover request timed out"))?
            .map_err(|error| CatalogProviderError::new(error.to_string()))?;
            if !response.status().is_success() {
                return Err(CatalogProviderError::new(format!("catalog cover returned HTTP {}", response.status())));
            }
            if response.content_length().is_some_and(|length| length > MAX_CATALOG_COVER_SOURCE_BYTES) {
                return Err(CatalogProviderError::new("catalog cover is too large"));
            }
            let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(|value| value.split(';').next()).unwrap_or("");
            if media_type == "image/svg+xml" || !(media_type.is_empty() || media_type.starts_with("image/") || media_type == "application/octet-stream") {
                return Err(CatalogProviderError::new("catalog cover has an unsupported media type"));
            }
            let mut source = Vec::with_capacity(response.content_length().unwrap_or(0).min(MAX_CATALOG_COVER_SOURCE_BYTES) as usize);
            let mut body = response.bytes_stream();
            while let Some(chunk) = body.next().await {
                let chunk = chunk.map_err(|error| CatalogProviderError::new(error.to_string()))?;
                if source.len().saturating_add(chunk.len()) as u64 > MAX_CATALOG_COVER_SOURCE_BYTES {
                    return Err(CatalogProviderError::new("catalog cover is too large"));
                }
                source.extend_from_slice(&chunk);
            }
            if source.is_empty() {
                return Err(CatalogProviderError::new("catalog cover is empty or too large"));
            }
            Ok(source)
        }

        #[cfg(not(target_arch = "wasm32"))]
        async fn publish_cover(&self, path: &Path, bytes: &[u8]) -> Result<(), CatalogProviderError> {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|error| CatalogProviderError::new(error.to_string()))?;
            }
            let temporary = self.directory.join(format!(".catalog-cover-{}.tmp", uuid::Uuid::new_v4()));
            let result = async {
                let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).await?;
                file.write_all(bytes).await?;
                file.flush().await?;
                file.sync_all().await?;
                tokio::fs::rename(&temporary, path).await
            }
            .await;
            if let Err(error) = result {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(CatalogProviderError::new(error.to_string()));
            }
            Ok(())
        }
    }

    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    impl CatalogCoverPipeline {
        pub fn new(cache: CatalogCoverCache, download_workers: usize, thumbnail_workers: usize) -> Result<Self, CatalogProviderError> {
            let runtime = tokio::runtime::Handle::try_current().map_err(|_| CatalogProviderError::new("catalog cover pipeline requires a Tokio runtime"))?;
            let (download_sender, download_receiver) = mpsc::channel::<CatalogCoverDownload>(COVER_DOWNLOAD_QUEUE_CAPACITY);
            let (thumbnail_sender, thumbnail_receiver) = mpsc::channel::<CatalogCoverResize>(COVER_RESIZE_QUEUE_CAPACITY);
            let download_receiver = Arc::new(AsyncMutex::new(download_receiver));
            let thumbnail_receiver = Arc::new(AsyncMutex::new(thumbnail_receiver));
            let scheduled = Arc::new(Mutex::new(HashSet::new()));

            for _ in 0..download_workers.max(1) {
                let cache = cache.clone();
                let receiver = download_receiver.clone();
                let thumbnails = thumbnail_sender.clone();
                let scheduled = scheduled.clone();
                runtime.spawn(async move {
                    loop {
                        let job = receiver.lock().await.recv().await;
                        let Some(job) = job else { break };
                        match cache.fetch_cover_source(&job.image_url).await {
                            Ok(source) => {
                                if thumbnails.send(CatalogCoverResize { image_url: job.image_url.clone(), source }).await.is_err() {
                                    scheduled.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&cover_cache_key(&job.image_url));
                                    break;
                                }
                            }
                            Err(error) => {
                                scheduled.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&cover_cache_key(&job.image_url));
                                log::debug!(target: "catalog", "cover download failed for {} from {}: {error}", job.image_url, job.source_id.as_str());
                            }
                        }
                    }
                });
            }
            drop(thumbnail_sender);

            for _ in 0..thumbnail_workers.max(1) {
                let cache = cache.clone();
                let receiver = thumbnail_receiver.clone();
                let scheduled = scheduled.clone();
                runtime.spawn(async move {
                    loop {
                        let job = receiver.lock().await.recv().await;
                        let Some(job) = job else { break };
                        let image_url = job.image_url.clone();
                        let normalized = tokio::task::spawn_blocking(move || normalize_cover(&job.source)).await;
                        let result = match normalized {
                            Ok(Ok(bytes)) => cache.publish_cover(&cover_path(&cache.directory, &image_url), &bytes).await,
                            Ok(Err(error)) => Err(error),
                            Err(error) => Err(CatalogProviderError::new(error.to_string())),
                        };
                        if let Err(error) = result {
                            if error.is_unsuitable_cover_dimensions() {
                                cache.unsuitable.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(cover_cache_key(&image_url));
                            }
                            log::debug!(target: "catalog", "cover thumbnail generation failed for {image_url}: {error}");
                        } else {
                            cache.unsuitable.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&cover_cache_key(&image_url));
                        }
                        scheduled.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&cover_cache_key(&image_url));
                    }
                });
            }

            Ok(Self { cache, downloads: download_sender, scheduled })
        }

        pub async fn enqueue(&self, source_id: CatalogSourceId, image_urls: impl IntoIterator<Item = String>) {
            for image_url in image_urls.into_iter().filter(|image_url| image_url.len() <= MAX_CATALOG_COVER_URL_BYTES) {
                self.cache.remember_cover_url(&source_id, &image_url);
                if self.cache.unsuitable.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).contains(&cover_cache_key(&image_url)) {
                    continue;
                }
                if read_cached_cover(&cover_path(&self.cache.directory, &image_url)).await.is_some() {
                    continue;
                }
                let cache_key = cover_cache_key(&image_url);
                if !self.scheduled.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(cache_key.clone()) {
                    continue;
                }
                if self.downloads.try_send(CatalogCoverDownload { source_id: source_id.clone(), image_url }).is_err() {
                    self.scheduled.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&cache_key);
                }
            }
        }
    }

    #[cfg(feature = "cover-cache")]
    fn cover_reference_key(source_id: &CatalogSourceId, image_url: &str) -> String {
        format!("{}\n{image_url}", source_id.as_str())
    }

    #[cfg(feature = "cover-cache")]
    fn cover_cache_key(image_url: &str) -> String {
        blake3::hash(image_url.as_bytes()).to_hex().to_string()
    }

    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    fn cover_path(directory: &Path, image_url: &str) -> PathBuf {
        directory.join(format!("{}.jpg", cover_cache_key(image_url)))
    }

    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    fn cover_asset_path(directory: &Path, digest: &str) -> PathBuf {
        directory.join("assets").join(format!("{digest}.jpg"))
    }

    #[cfg(feature = "cover-cache")]
    fn catalog_cover_asset_reference(digest: &str) -> String {
        format!("{CATALOG_COVER_ASSET_PATH_PREFIX}{digest}.jpg")
    }

    #[cfg(feature = "cover-cache")]
    pub fn catalog_cover_asset_digest(reference: &str) -> Option<&str> {
        let digest = reference.strip_prefix(CATALOG_COVER_ASSET_PATH_PREFIX)?.strip_suffix(".jpg")?;
        valid_cover_asset_digest(digest).then_some(digest)
    }

    #[cfg(feature = "cover-cache")]
    fn valid_cover_asset_digest(digest: &str) -> bool {
        digest.len() == blake3::OUT_LEN * 2 && digest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }

    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    fn normalize_cover(bytes: &[u8]) -> Result<Vec<u8>, CatalogProviderError> {
        let dimensions = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|error| CatalogProviderError::new(error.to_string()))?.into_dimensions().map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if !thumbnail::reasonable_cover_dimensions(dimensions.0, dimensions.1) {
            return Err(CatalogProviderError::unsuitable_cover_dimensions(dimensions.0, dimensions.1));
        }
        if dimensions.0 > MAX_CATALOG_COVER_DECODE_WIDTH || dimensions.1 > MAX_CATALOG_COVER_DECODE_HEIGHT {
            return Err(CatalogProviderError::new(format!("catalog cover dimensions {}x{} exceed the decode limit", dimensions.0, dimensions.1)));
        }
        let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|error| CatalogProviderError::new(error.to_string()))?;
        let mut limits = image::io::Limits::default();
        // A cover is immediately reduced to 408x612. Allowing four concurrent
        // 128 MiB decodes made the server retain a >1 GiB anonymous heap after a
        // crawl and enter permanent cgroup reclaim. Larger sources are not useful
        // enough to justify making the catalogue unavailable.
        limits.max_image_width = Some(MAX_CATALOG_COVER_DECODE_WIDTH);
        limits.max_image_height = Some(MAX_CATALOG_COVER_DECODE_HEIGHT);
        limits.max_alloc = Some(MAX_CATALOG_COVER_DECODE_ALLOC);
        reader.limits(limits);
        let image = reader.decode().map_err(|error| CatalogProviderError::new(format!("invalid catalog cover: {error}")))?;
        let thumbnail = image.thumbnail(CATALOG_COVER_WIDTH, CATALOG_COVER_HEIGHT).to_rgb8();
        let mut normalized = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(thumbnail).write_to(&mut normalized, image::ImageOutputFormat::Jpeg(85)).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        Ok(normalized.into_inner())
    }

    #[doc(hidden)]
    #[cfg(feature = "cover-cache")]
    pub fn validate_server_cover(bytes: Vec<u8>) -> Result<Vec<u8>, CatalogProviderError> {
        let mut reader = ImageReader::new(Cursor::new(bytes.as_slice())).with_guessed_format().map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if reader.format() != Some(image::ImageFormat::Jpeg) {
            return Err(CatalogProviderError::new("catalog server cover body is not a JPEG"));
        }
        let mut limits = image::io::Limits::default();
        limits.max_image_width = Some(MAX_SERVER_COVER_WIDTH);
        limits.max_image_height = Some(MAX_SERVER_COVER_HEIGHT);
        limits.max_alloc = Some(16 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().map_err(|error| CatalogProviderError::new(format!("invalid catalog server cover: {error}")))?;
        if image.width() == 0 || image.height() == 0 {
            return Err(CatalogProviderError::new("catalog server cover has invalid dimensions"));
        }
        Ok(bytes)
    }

    #[cfg(all(feature = "cover-cache", not(target_arch = "wasm32")))]
    async fn read_cached_cover(path: &Path) -> Option<Vec<u8>> {
        let metadata = tokio::fs::metadata(path).await.ok()?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_CATALOG_COVER_SOURCE_BYTES {
            return None;
        }
        let bytes = tokio::fs::read(path).await.ok()?;
        let dimensions = ImageReader::new(Cursor::new(bytes.as_slice())).with_guessed_format().ok()?.into_dimensions().ok()?;
        thumbnail::reasonable_cover_dimensions(dimensions.0, dimensions.1).then_some(bytes)
    }

    #[cfg(all(test, feature = "cover-cache", not(target_arch = "wasm32")))]
    mod cover_tests {
        use super::*;
        use image::ImageOutputFormat;

        fn png(width: u32, height: u32) -> Vec<u8> {
            let mut bytes = Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(width, height).write_to(&mut bytes, ImageOutputFormat::Png).unwrap();
            bytes.into_inner()
        }

        #[test]
        fn catalog_cover_normalization_rejects_tiny_and_non_cover_images() {
            assert!(normalize_cover(&png(32, 48)).is_err());
            assert!(normalize_cover(&png(800, 600)).is_err());
            assert!(normalize_cover(&png(320, 480)).is_ok());
        }

        #[test]
        fn remembered_cover_sets_evict_old_entries_at_their_bound() {
            let mut set = BoundedStringSet::new(2);
            assert!(set.insert("one".to_owned()));
            assert!(set.insert("two".to_owned()));
            assert!(set.insert("three".to_owned()));
            assert!(!set.contains("one"));
            assert!(set.contains("two"));
            assert!(set.contains("three"));
            assert!(set.remove("two"));
            assert!(set.insert("two".to_owned()));
            assert!(set.contains("two"));
        }

        #[tokio::test]
        async fn cached_covers_are_promoted_to_content_addressed_assets() {
            let temporary = tempfile::tempdir().unwrap();
            let cache = CatalogCoverCache::new(temporary.path().to_owned()).unwrap();
            let source_id = CatalogSourceId::parse("opds-test").unwrap();
            let image_url = "https://example.com/cover.png";
            cache.remember_cover_url(&source_id, image_url);
            let normalized = normalize_cover(&png(320, 480)).unwrap();
            cache.cover_with(&source_id, image_url, async { Ok(normalized.clone()) }).await.unwrap();

            let reference = cache.public_cover_reference(image_url).await.unwrap();
            let digest = catalog_cover_asset_digest(&reference).unwrap();
            assert_eq!(cache.cached_cover_asset(digest).await.unwrap(), normalized);
            assert_eq!(cache.public_cover_reference(image_url).await.as_deref(), Some(reference.as_str()));
        }
    }
}

#[cfg(feature = "provider")]
pub use provider::*;
