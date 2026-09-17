mod covers {
    use catalog_model::{CatalogSourceId, CatalogSubjectBrowseResponse};
    use image::io::Reader as ImageReader;
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::fmt;
    use std::future::Future;
    use std::io::Cursor;
    #[cfg(not(target_arch = "wasm32"))]
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, Weak};
    #[cfg(not(target_arch = "wasm32"))]
    use tokio::sync::mpsc;
    use tokio::sync::{Mutex as AsyncMutex, Semaphore};

    // Keep the queued compressed-image working set small. Catalogue crawls can
    // discover thousands of covers faster than the CPU can decode them, and each
    // queued body is retained until a resize worker receives it.
    #[cfg(not(target_arch = "wasm32"))]
    const MAX_CATALOG_COVER_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
    #[cfg(not(target_arch = "wasm32"))]
    const MAX_CATALOG_COVER_DECODE_WIDTH: u32 = 2048;
    #[cfg(not(target_arch = "wasm32"))]
    const MAX_CATALOG_COVER_DECODE_HEIGHT: u32 = 3072;
    #[cfg(not(target_arch = "wasm32"))]
    const MAX_CATALOG_COVER_DECODE_ALLOC: u64 = 28 * 1024 * 1024;
    #[cfg(not(target_arch = "wasm32"))]
    const CATALOG_COVER_WIDTH: u32 = 408;
    #[cfg(not(target_arch = "wasm32"))]
    const CATALOG_COVER_HEIGHT: u32 = 612;
    // Embedded EPUB thumbnails use the library thumbnail contract (600 px wide)
    // and may therefore be larger than covers normalized from upstream URLs.
    const MAX_SERVER_COVER_WIDTH: u32 = 1200;
    const MAX_SERVER_COVER_HEIGHT: u32 = 1920;
    const MAX_CATALOG_COVER_URL_BYTES: usize = 4096;
    const MAX_CONCURRENT_CATALOG_COVERS: usize = 4;
    const MAX_REMEMBERED_COVER_REFERENCES: usize = 8_192;
    const MAX_REMEMBERED_UNSUITABLE_COVERS: usize = 8_192;
    #[cfg(not(target_arch = "wasm32"))]
    const COVER_DOWNLOAD_QUEUE_CAPACITY: usize = 1_024;
    #[cfg(not(target_arch = "wasm32"))]
    const COVER_RESIZE_QUEUE_CAPACITY: usize = 4;
    const CATALOG_COVER_ASSET_PATH_PREFIX: &str = "/api/opds/covers/";

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum CatalogCoverErrorKind {
        Other,
        UnsuitableCoverDimensions,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct CatalogCoverError {
        message: String,
        kind: CatalogCoverErrorKind,
    }

    impl CatalogCoverError {
        pub fn new(message: impl fmt::Display) -> Self {
            Self { message: message.to_string(), kind: CatalogCoverErrorKind::Other }
        }

        #[cfg(not(target_arch = "wasm32"))]
        fn unsuitable_cover_dimensions(width: u32, height: u32) -> Self {
            Self { message: format!("catalog cover dimensions {width}x{height} are not suitable for a book cover"), kind: CatalogCoverErrorKind::UnsuitableCoverDimensions }
        }

        fn known_unsuitable_cover_dimensions() -> Self {
            Self { message: "catalog cover dimensions are not suitable for a book cover".to_owned(), kind: CatalogCoverErrorKind::UnsuitableCoverDimensions }
        }

        pub fn is_unsuitable_cover_dimensions(&self) -> bool {
            self.kind == CatalogCoverErrorKind::UnsuitableCoverDimensions
        }
    }

    impl fmt::Display for CatalogCoverError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(&self.message)
        }
    }

    impl std::error::Error for CatalogCoverError {}

    #[cfg(not(target_arch = "wasm32"))]
    mod cover_storage {
        use super::*;
        use tokio::io::AsyncWriteExt;

        #[derive(Clone)]
        pub(super) struct Storage {
            directory: Arc<PathBuf>,
        }

        impl Storage {
            pub(super) fn open(directory: PathBuf) -> Result<Self, CatalogCoverError> {
                std::fs::create_dir_all(&directory).map_err(CatalogCoverError::new)?;
                let directory = directory.canonicalize().map_err(CatalogCoverError::new)?;
                Ok(Self { directory: Arc::new(directory) })
            }

            pub(super) async fn read_cover(&self, image_url: &str) -> Option<Vec<u8>> {
                read_cached_cover(&cover_path(&self.directory, image_url)).await
            }

            pub(super) async fn write_cover(&self, image_url: &str, bytes: &[u8]) -> Result<(), CatalogCoverError> {
                self.publish(&cover_path(&self.directory, image_url), bytes).await
            }

            pub(super) async fn read_asset(&self, digest: &str) -> Option<Vec<u8>> {
                read_cached_cover(&cover_asset_path(&self.directory, digest)).await
            }

            pub(super) async fn write_asset(&self, digest: &str, bytes: &[u8]) -> Result<(), CatalogCoverError> {
                self.publish(&cover_asset_path(&self.directory, digest), bytes).await
            }

            async fn publish(&self, path: &Path, bytes: &[u8]) -> Result<(), CatalogCoverError> {
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent).await.map_err(CatalogCoverError::new)?;
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
                    return Err(CatalogCoverError::new(error));
                }
                Ok(())
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    mod cover_storage {
        use super::*;

        #[derive(Clone)]
        pub(super) struct Storage {
            cached: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        }

        impl Storage {
            pub(super) fn open(_directory: PathBuf) -> Result<Self, CatalogCoverError> {
                Ok(Self { cached: Arc::new(Mutex::new(HashMap::new())) })
            }

            pub(super) async fn read_cover(&self, image_url: &str) -> Option<Vec<u8>> {
                self.cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&format!("url\n{image_url}")).cloned()
            }

            pub(super) async fn write_cover(&self, image_url: &str, bytes: &[u8]) -> Result<(), CatalogCoverError> {
                self.cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(format!("url\n{image_url}"), bytes.to_vec());
                Ok(())
            }

            pub(super) async fn read_asset(&self, digest: &str) -> Option<Vec<u8>> {
                self.cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&format!("asset\n{digest}")).cloned()
            }

            pub(super) async fn write_asset(&self, digest: &str, bytes: &[u8]) -> Result<(), CatalogCoverError> {
                self.cached.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(format!("asset\n{digest}"), bytes.to_vec());
                Ok(())
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    mod cover_fetch {
        use super::*;
        use futures_util::StreamExt;
        use std::time::Duration;
        use url::Url;

        #[derive(Clone)]
        pub(super) struct Client {
            policy: outbound::OutboundNetworkPolicy,
        }

        impl Client {
            pub(super) fn allow_private() -> Self {
                Self { policy: outbound::OutboundNetworkPolicy::AllowPrivate }
            }

            pub(super) fn public_only() -> Self {
                Self { policy: outbound::OutboundNetworkPolicy::PublicOnly }
            }

            pub(super) async fn fetch(&self, image_url: &str) -> Result<Vec<u8>, CatalogCoverError> {
                let url = Url::parse(image_url).map_err(CatalogCoverError::new)?;
                if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
                    return Err(CatalogCoverError::new("catalog cover URL must be absolute credential-free HTTP(S)"));
                }
                let response = tokio::time::timeout(
                    Duration::from_secs(15),
                    outbound::send_get(url, reqwest::header::HeaderMap::new(), self.policy, &format!("Bokheim/{} catalog-cover-client", env!("CARGO_PKG_VERSION")), Duration::from_secs(5), Duration::from_secs(15)),
                )
                .await
                .map_err(|_| CatalogCoverError::new("catalog cover request timed out"))?
                .map_err(CatalogCoverError::new)?;
                if !response.status().is_success() {
                    return Err(CatalogCoverError::new(format!("catalog cover returned HTTP {}", response.status())));
                }
                if response.content_length().is_some_and(|length| length > MAX_CATALOG_COVER_SOURCE_BYTES) {
                    return Err(CatalogCoverError::new("catalog cover is too large"));
                }
                let media_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).and_then(|value| value.split(';').next()).unwrap_or("");
                if media_type == "image/svg+xml" || !(media_type.is_empty() || media_type.starts_with("image/") || media_type == "application/octet-stream") {
                    return Err(CatalogCoverError::new("catalog cover has an unsupported media type"));
                }
                let mut source = Vec::with_capacity(response.content_length().unwrap_or(0).min(MAX_CATALOG_COVER_SOURCE_BYTES) as usize);
                let mut body = response.bytes_stream();
                while let Some(chunk) = body.next().await {
                    let chunk = chunk.map_err(CatalogCoverError::new)?;
                    if source.len().saturating_add(chunk.len()) as u64 > MAX_CATALOG_COVER_SOURCE_BYTES {
                        return Err(CatalogCoverError::new("catalog cover is too large"));
                    }
                    source.extend_from_slice(&chunk);
                }
                if source.is_empty() {
                    return Err(CatalogCoverError::new("catalog cover is empty or too large"));
                }
                Ok(source)
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    mod cover_fetch {
        #[derive(Clone)]
        pub(super) struct Client;

        impl Client {
            pub(super) fn allow_private() -> Self {
                Self
            }
        }
    }

    #[derive(Clone)]
    pub struct CatalogCoverCache {
        _fetcher: cover_fetch::Client,
        storage: cover_storage::Storage,
        known: Arc<Mutex<BoundedStringSet>>,
        unsuitable: Arc<Mutex<BoundedStringSet>>,
        in_flight: Arc<Mutex<HashMap<String, Weak<AsyncMutex<()>>>>>,
        downloads: Arc<Semaphore>,
    }

    #[derive(Clone)]
    #[cfg(not(target_arch = "wasm32"))]
    pub struct CatalogCoverPipeline {
        cache: CatalogCoverCache,
        downloads: mpsc::Sender<CatalogCoverDownload>,
        scheduled: Arc<Mutex<HashSet<String>>>,
    }

    #[cfg(not(target_arch = "wasm32"))]
    struct CatalogCoverDownload {
        source_id: CatalogSourceId,
        image_url: String,
    }

    #[cfg(not(target_arch = "wasm32"))]
    struct CatalogCoverResize {
        image_url: String,
        source: Vec<u8>,
    }

    struct BoundedStringSet {
        values: HashSet<String>,
        order: VecDeque<String>,
        capacity: usize,
    }

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

    /// Shared cover cache used by catalogue clients and the catalogue server.
    impl CatalogCoverCache {
        pub fn new(directory: PathBuf) -> Result<Self, CatalogCoverError> {
            Self::build(directory, cover_fetch::Client::allow_private())
        }

        #[cfg(not(target_arch = "wasm32"))]
        /// Creates the cover cache used by an Internet-facing service. Cover URLs
        /// and every redirect hop are restricted to publicly routed addresses.
        pub fn new_public_only(directory: PathBuf) -> Result<Self, CatalogCoverError> {
            Self::build(directory, cover_fetch::Client::public_only())
        }

        fn build(directory: PathBuf, fetcher: cover_fetch::Client) -> Result<Self, CatalogCoverError> {
            Ok(Self {
                _fetcher: fetcher,
                storage: cover_storage::Storage::open(directory)?,
                known: Arc::new(Mutex::new(BoundedStringSet::new(MAX_REMEMBERED_COVER_REFERENCES))),
                unsuitable: Arc::new(Mutex::new(BoundedStringSet::new(MAX_REMEMBERED_UNSUITABLE_COVERS))),
                in_flight: Arc::new(Mutex::new(HashMap::new())),
                downloads: Arc::new(Semaphore::new(MAX_CONCURRENT_CATALOG_COVERS)),
            })
        }

        pub fn remember_cover_url(&self, source_id: &CatalogSourceId, image_url: &str) {
            self.remember_cover_urls(std::iter::once((source_id, image_url)));
        }

        pub fn remember_subject_browse(&self, response: &CatalogSubjectBrowseResponse) {
            self.remember_cover_urls(response.books.iter().filter_map(|item| item.book.image_url.as_deref().map(|image_url| (&item.source_id, image_url))));
        }

        fn remember_cover_urls<'a>(&self, references: impl IntoIterator<Item = (&'a CatalogSourceId, &'a str)>) {
            let mut known = self.known.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            for (source_id, image_url) in references {
                if image_url.len() <= MAX_CATALOG_COVER_URL_BYTES {
                    known.insert(cover_reference_key(source_id, image_url));
                }
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub async fn cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogCoverError> {
            self.cover_with(source_id, image_url, self.fetch_cover(image_url)).await
        }

        /// Reads an already generated JPEG without contacting the upstream OPDS
        /// host. The cache server uses this on its request path; missing covers are
        /// retried by the next crawl instead of delaying a user request.
        pub async fn cached_cover(&self, image_url: &str) -> Result<Vec<u8>, CatalogCoverError> {
            if image_url.len() > MAX_CATALOG_COVER_URL_BYTES {
                return Err(CatalogCoverError::new("catalog cover URL is too long"));
            }
            if let Some(bytes) = self.storage.read_cover(image_url).await {
                return Ok(bytes);
            }
            if self.unsuitable.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).contains(&cover_cache_key(image_url)) {
                return Err(CatalogCoverError::known_unsuitable_cover_dimensions());
            }
            Err(CatalogCoverError::new("catalog cover thumbnail is not cached"))
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
            let bytes = self.storage.read_cover(image_url).await?;
            let digest = blake3::hash(&bytes).to_hex().to_string();
            if self.storage.read_asset(&digest).await.is_none() && self.storage.write_asset(&digest, &bytes).await.is_err() {
                return None;
            }
            Some(catalog_cover_asset_reference(&digest))
        }

        /// Reads an immutable cover by its normalized JPEG content digest.
        pub async fn cached_cover_asset(&self, digest: &str) -> Result<Vec<u8>, CatalogCoverError> {
            if !valid_cover_asset_digest(digest) {
                return Err(CatalogCoverError::new("catalog cover asset digest is invalid"));
            }
            let bytes = self.storage.read_asset(digest).await.ok_or_else(|| CatalogCoverError::new("catalog cover asset is not cached"))?;
            if blake3::hash(&bytes).to_hex().as_str() != digest {
                return Err(CatalogCoverError::new("catalog cover asset does not match its digest"));
            }
            Ok(bytes)
        }

        #[doc(hidden)]
        pub async fn cover_with(&self, source_id: &CatalogSourceId, image_url: &str, fetch: impl Future<Output = Result<Vec<u8>, CatalogCoverError>>) -> Result<Vec<u8>, CatalogCoverError> {
            if image_url.len() > MAX_CATALOG_COVER_URL_BYTES {
                return Err(CatalogCoverError::new("catalog cover URL is too long"));
            }
            let reference_key = cover_reference_key(source_id, image_url);
            let cache_key = cover_cache_key(image_url);
            if let Some(bytes) = self.storage.read_cover(image_url).await {
                return Ok(bytes);
            }
            if !self.known.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).contains(&reference_key) {
                return Err(CatalogCoverError::new("catalog cover URL was not present in a returned catalog page"));
            }
            let lock = {
                let mut in_flight = self.in_flight.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                in_flight.retain(|_, lock| lock.strong_count() > 0);
                if let Some(lock) = in_flight.get(&cache_key).and_then(Weak::upgrade) {
                    lock
                } else {
                    let lock = Arc::new(AsyncMutex::new(()));
                    in_flight.insert(cache_key.clone(), Arc::downgrade(&lock));
                    lock
                }
            };
            let _fetch = lock.lock().await;
            if let Some(bytes) = self.storage.read_cover(image_url).await {
                return Ok(bytes);
            }
            let _download = self.downloads.acquire().await.map_err(CatalogCoverError::new)?;
            let result = match fetch.await {
                Ok(bytes) => self.storage.write_cover(image_url, &bytes).await.map(|()| bytes),
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
            result
        }

        #[cfg(not(target_arch = "wasm32"))]
        async fn fetch_cover(&self, image_url: &str) -> Result<Vec<u8>, CatalogCoverError> {
            let source = self._fetcher.fetch(image_url).await?;
            tokio::task::spawn_blocking(move || normalize_cover(&source)).await.map_err(CatalogCoverError::new)?
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    impl CatalogCoverPipeline {
        pub fn new(cache: CatalogCoverCache, download_workers: usize, thumbnail_workers: usize) -> Result<Self, CatalogCoverError> {
            let runtime = tokio::runtime::Handle::try_current().map_err(|_| CatalogCoverError::new("catalog cover pipeline requires a Tokio runtime"))?;
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
                        match cache._fetcher.fetch(&job.image_url).await {
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
                            Ok(Ok(bytes)) => cache.storage.write_cover(&image_url, &bytes).await,
                            Ok(Err(error)) => Err(error),
                            Err(error) => Err(CatalogCoverError::new(error)),
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
                if self.cache.storage.read_cover(&image_url).await.is_some() {
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

    fn cover_reference_key(source_id: &CatalogSourceId, image_url: &str) -> String {
        format!("{}\n{image_url}", source_id.as_str())
    }

    fn cover_cache_key(image_url: &str) -> String {
        blake3::hash(image_url.as_bytes()).to_hex().to_string()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn cover_path(directory: &Path, image_url: &str) -> PathBuf {
        directory.join(format!("{}.jpg", cover_cache_key(image_url)))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn cover_asset_path(directory: &Path, digest: &str) -> PathBuf {
        directory.join("assets").join(format!("{digest}.jpg"))
    }

    fn catalog_cover_asset_reference(digest: &str) -> String {
        format!("{CATALOG_COVER_ASSET_PATH_PREFIX}{digest}.jpg")
    }

    pub fn catalog_cover_asset_digest(reference: &str) -> Option<&str> {
        let digest = reference.strip_prefix(CATALOG_COVER_ASSET_PATH_PREFIX)?.strip_suffix(".jpg")?;
        valid_cover_asset_digest(digest).then_some(digest)
    }

    fn valid_cover_asset_digest(digest: &str) -> bool {
        digest.len() == blake3::OUT_LEN * 2 && digest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn normalize_cover(bytes: &[u8]) -> Result<Vec<u8>, CatalogCoverError> {
        let dimensions = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(CatalogCoverError::new)?.into_dimensions().map_err(CatalogCoverError::new)?;
        if !thumbnail::reasonable_cover_dimensions(dimensions.0, dimensions.1) {
            return Err(CatalogCoverError::unsuitable_cover_dimensions(dimensions.0, dimensions.1));
        }
        if dimensions.0 > MAX_CATALOG_COVER_DECODE_WIDTH || dimensions.1 > MAX_CATALOG_COVER_DECODE_HEIGHT {
            return Err(CatalogCoverError::new(format!("catalog cover dimensions {}x{} exceed the decode limit", dimensions.0, dimensions.1)));
        }
        let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(CatalogCoverError::new)?;
        let mut limits = image::io::Limits::default();
        // A cover is immediately reduced to 408x612. Allowing four concurrent
        // 128 MiB decodes made the server retain a >1 GiB anonymous heap after a
        // crawl and enter permanent cgroup reclaim. Larger sources are not useful
        // enough to justify making the catalogue unavailable.
        limits.max_image_width = Some(MAX_CATALOG_COVER_DECODE_WIDTH);
        limits.max_image_height = Some(MAX_CATALOG_COVER_DECODE_HEIGHT);
        limits.max_alloc = Some(MAX_CATALOG_COVER_DECODE_ALLOC);
        reader.limits(limits);
        let image = reader.decode().map_err(|error| CatalogCoverError::new(format!("invalid catalog cover: {error}")))?;
        let thumbnail = image.thumbnail(CATALOG_COVER_WIDTH, CATALOG_COVER_HEIGHT).to_rgb8();
        let mut normalized = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(thumbnail).write_to(&mut normalized, image::ImageOutputFormat::Jpeg(85)).map_err(CatalogCoverError::new)?;
        Ok(normalized.into_inner())
    }

    #[doc(hidden)]
    pub fn validate_server_cover(bytes: Vec<u8>) -> Result<Vec<u8>, CatalogCoverError> {
        let mut reader = ImageReader::new(Cursor::new(bytes.as_slice())).with_guessed_format().map_err(CatalogCoverError::new)?;
        if reader.format() != Some(image::ImageFormat::Jpeg) {
            return Err(CatalogCoverError::new("catalog server cover body is not a JPEG"));
        }
        let mut limits = image::io::Limits::default();
        limits.max_image_width = Some(MAX_SERVER_COVER_WIDTH);
        limits.max_image_height = Some(MAX_SERVER_COVER_HEIGHT);
        limits.max_alloc = Some(16 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().map_err(|error| CatalogCoverError::new(format!("invalid catalog server cover: {error}")))?;
        if image.width() == 0 || image.height() == 0 {
            return Err(CatalogCoverError::new("catalog server cover has invalid dimensions"));
        }
        Ok(bytes)
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn read_cached_cover(path: &Path) -> Option<Vec<u8>> {
        let metadata = tokio::fs::metadata(path).await.ok()?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_CATALOG_COVER_SOURCE_BYTES {
            return None;
        }
        let bytes = tokio::fs::read(path).await.ok()?;
        let dimensions = ImageReader::new(Cursor::new(bytes.as_slice())).with_guessed_format().ok()?.into_dimensions().ok()?;
        thumbnail::reasonable_cover_dimensions(dimensions.0, dimensions.1).then_some(bytes)
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
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
        async fn failed_cover_requests_remain_serialized_while_waiters_exist() {
            use futures_util::{stream, StreamExt};
            use std::sync::atomic::{AtomicUsize, Ordering};

            let temporary = tempfile::tempdir().unwrap();
            let cache = CatalogCoverCache::new(temporary.path().to_owned()).unwrap();
            let source_id = CatalogSourceId::parse("opds-test").unwrap();
            let image_url = "https://example.com/failing-cover.png";
            cache.remember_cover_url(&source_id, image_url);
            let active = AtomicUsize::new(0);
            // Each completion admits a new caller while older callers still wait.
            let requests = stream::iter(0..12)
                .map(|_| {
                    cache.cover_with(&source_id, image_url, async {
                        assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0, "one cover must have only one active fetch");
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                        active.fetch_sub(1, Ordering::SeqCst);
                        Err(CatalogCoverError::new("injected failure"))
                    })
                })
                .buffer_unordered(3)
                .collect::<Vec<_>>();
            let results = tokio::time::timeout(std::time::Duration::from_secs(2), requests).await.unwrap();
            assert!(results.iter().all(Result::is_err));
            assert_eq!(active.load(Ordering::SeqCst), 0);
            assert!(cache.in_flight.lock().unwrap().values().all(|lock| lock.upgrade().is_none()));
        }

        #[tokio::test]
        async fn cancelled_cover_request_releases_lock_and_download_permit() {
            let temporary = tempfile::tempdir().unwrap();
            let cache = CatalogCoverCache::new(temporary.path().to_owned()).unwrap();
            let source_id = CatalogSourceId::parse("opds-test").unwrap();
            let image_url = "https://example.com/cancelled-cover.png";
            cache.remember_cover_url(&source_id, image_url);
            let (started, ready) = tokio::sync::oneshot::channel();
            let mut request = Box::pin(cache.cover_with(&source_id, image_url, async {
                started.send(()).unwrap();
                std::future::pending().await
            }));
            tokio::select! {
                result = ready => result.unwrap(),
                result = &mut request => panic!("fetch should remain pending: {result:?}"),
            }
            drop(request);
            assert!(cache.in_flight.lock().unwrap().values().all(|lock| lock.upgrade().is_none()));
            assert_eq!(cache.downloads.available_permits(), MAX_CONCURRENT_CATALOG_COVERS);
            let normalized = normalize_cover(&png(320, 480)).unwrap();
            let retry = cache.cover_with(&source_id, image_url, async { Ok(normalized.clone()) });
            let bytes = tokio::time::timeout(std::time::Duration::from_secs(2), retry).await.unwrap().unwrap();
            assert_eq!(bytes, normalized);
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

pub use covers::*;
