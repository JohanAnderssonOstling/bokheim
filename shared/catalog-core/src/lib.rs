mod cache;
mod language;

pub use cache::{
    CachedCatalogPage, CatalogCache, CatalogCacheError, CatalogCrawlGeneration, CatalogEpubExtractionInventory, CatalogEpubExtractionJob, CatalogEpubMetadata, CatalogInventory, CatalogMetadataBackfillInventory, CatalogMetadataBackfillJob,
    CatalogMetadataBackfillUpdate, CatalogPublicationPopularity, CatalogSearchMatches, CatalogSourceInventory, CatalogSourceState, SequencedCatalogPublication,
};
pub use language::CatalogLanguageFilter;

use catalog_contract::{
    CatalogAuthentication, CatalogAuthorCredit, CatalogAuthorDiscoveryEvidence, CatalogAuthorDiscoveryMatch, CatalogAuthorDiscoveryRequest, CatalogAuthorDiscoveryResult, CatalogBookFormat, CatalogCapability, CatalogDescription,
    CatalogExternalActionKind, CatalogFacet, CatalogFreshness, CatalogGroup, CatalogImportRequest, CatalogLink, CatalogLinkTarget, CatalogPage, CatalogPagination, CatalogPrice, CatalogPublication, CatalogPublicationAction,
    CatalogPublicationIdentifier, CatalogPublicationIdentity, CatalogRecommendationResponse, CatalogSearch, CatalogSearchDefinition, CatalogSearchField, CatalogSearchQuery, CatalogSource, CatalogSourceId, CatalogSubjectBrowseResponse,
    CatalogWideSearchGroup, CatalogWideSearchOutcome, CatalogWideSearchResult,
};
use catalog_model::AuthorName;
use futures_util::stream::{self, StreamExt};
use image::io::Reader as ImageReader;
use opds::{
    OpdsAcquisition, OpdsAcquisitionKind, OpdsBookFormat, OpdsCatalog, OpdsCatalogLink, OpdsDescription, OpdsDirectAcquisition, OpdsEndpoint, OpdsEndpointId, OpdsFacet, OpdsGroup, OpdsLinkTarget, OpdsPrice, OpdsPublication,
    OpdsPublicationIdentity, OpdsSearch, OpdsSearchQuery,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::future::Future;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Mutex as AsyncMutex, Semaphore};
use url::Url;

const OPDS_SOURCE_PREFIX: &str = "opds-";
const MAX_WIDE_SEARCH_CONCURRENCY: usize = 16;
const WIDE_SEARCH_TIMEOUT: Duration = Duration::from_secs(8);
// Keep the queued compressed-image working set small. Catalogue crawls can
// discover thousands of covers faster than the CPU can decode them, and each
// queued body is retained until a resize worker receives it.
const MAX_CATALOG_COVER_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CATALOG_COVER_DECODE_WIDTH: u32 = 2048;
const MAX_CATALOG_COVER_DECODE_HEIGHT: u32 = 3072;
const MAX_CATALOG_COVER_DECODE_ALLOC: u64 = 28 * 1024 * 1024;
const CATALOG_COVER_WIDTH: u32 = 408;
const CATALOG_COVER_HEIGHT: u32 = 612;
// Embedded EPUB thumbnails use the library thumbnail contract (600 px wide)
// and may therefore be larger than covers normalized from upstream URLs.
const MAX_SERVER_COVER_WIDTH: u32 = 1200;
const MAX_SERVER_COVER_HEIGHT: u32 = 1920;
const MAX_CATALOG_COVER_URL_BYTES: usize = 4096;
const MAX_CONCURRENT_CATALOG_COVERS: usize = 4;
const MAX_REMEMBERED_COVER_REFERENCES: usize = 8_192;
const MAX_REMEMBERED_UNSUITABLE_COVERS: usize = 8_192;
const COVER_DOWNLOAD_QUEUE_CAPACITY: usize = 1_024;
const COVER_RESIZE_QUEUE_CAPACITY: usize = 4;
const MAX_AUTHOR_DISCOVERY_MATCHES: usize = 200;

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

#[derive(Clone)]
pub struct CatalogCoverCache {
    outbound_policy: opds::OutboundNetworkPolicy,
    directory: Arc<PathBuf>,
    known: Arc<Mutex<BoundedStringSet>>,
    unsuitable: Arc<Mutex<BoundedStringSet>>,
    in_flight: Arc<AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>>,
    downloads: Arc<Semaphore>,
}

#[derive(Clone)]
pub struct CatalogCoverPipeline {
    cache: CatalogCoverCache,
    downloads: mpsc::Sender<CatalogCoverDownload>,
    scheduled: Arc<Mutex<HashSet<String>>>,
}

struct CatalogCoverDownload {
    source_id: CatalogSourceId,
    image_url: String,
}

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

/// App-facing catalogue component. It composes the remote catalogue server
/// with direct OPDS fallback, endpoint persistence, and cover caching.
impl CatalogCoverCache {
    pub fn new(directory: PathBuf) -> Result<Self, CatalogProviderError> {
        Self::with_outbound_policy(directory, opds::OutboundNetworkPolicy::AllowPrivate)
    }

    /// Creates the cover cache used by an Internet-facing service. Cover URLs
    /// and every redirect hop are restricted to publicly routed addresses.
    pub fn new_public_only(directory: PathBuf) -> Result<Self, CatalogProviderError> {
        Self::with_outbound_policy(directory, opds::OutboundNetworkPolicy::PublicOnly)
    }

    fn with_outbound_policy(directory: PathBuf, outbound_policy: opds::OutboundNetworkPolicy) -> Result<Self, CatalogProviderError> {
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

    pub fn remember_author_discovery(&self, result: &CatalogAuthorDiscoveryResult) {
        let references = result.matches.iter().filter_map(|result| result.publication.image_url.as_ref().map(|image_url| (result.source.id.clone(), image_url.clone()))).collect();
        self.remember_cover_urls(references);
    }

    pub fn remember_recommendations(&self, response: &CatalogRecommendationResponse) {
        let references = response.recommendations.iter().filter_map(|recommendation| recommendation.publication.image_url.as_ref().map(|image_url| (recommendation.source_id.clone(), image_url.clone()))).collect();
        self.remember_cover_urls(references);
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

    pub fn remember_wide_search(&self, result: &CatalogWideSearchResult) {
        for group in &result.groups {
            if let CatalogWideSearchOutcome::Results { page } = &group.outcome {
                self.remember_page(page);
            }
        }
    }

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
        if let Some(bytes) = read_cached_cover(&cover_path(&self.directory, image_url)).await {
            return Ok(bytes);
        }
        if self.unsuitable.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).contains(&cover_cache_key(image_url)) {
            return Err(CatalogProviderError::known_unsuitable_cover_dimensions());
        }
        Err(CatalogProviderError::new("catalog cover thumbnail is not cached"))
    }

    #[doc(hidden)]
    pub async fn cover_with(&self, source_id: &CatalogSourceId, image_url: &str, fetch: impl Future<Output = Result<Vec<u8>, CatalogProviderError>>) -> Result<Vec<u8>, CatalogProviderError> {
        if image_url.len() > MAX_CATALOG_COVER_URL_BYTES {
            return Err(CatalogProviderError::new("catalog cover URL is too long"));
        }
        let reference_key = cover_reference_key(source_id, image_url);
        let cache_key = cover_cache_key(image_url);
        let path = cover_path(&self.directory, image_url);
        if let Some(bytes) = read_cached_cover(&path).await {
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
        if let Some(bytes) = read_cached_cover(&path).await {
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
            Ok(bytes) => self.publish_cover(&path, &bytes).await.map(|()| bytes),
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

    async fn fetch_cover(&self, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
        let source = self.fetch_cover_source(image_url).await?;
        tokio::task::spawn_blocking(move || normalize_cover(&source)).await.map_err(|error| CatalogProviderError::new(error.to_string()))?
    }

    async fn fetch_cover_source(&self, image_url: &str) -> Result<Vec<u8>, CatalogProviderError> {
        let url = Url::parse(image_url).map_err(|error| CatalogProviderError::new(error.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
            return Err(CatalogProviderError::new("catalog cover URL must be absolute credential-free HTTP(S)"));
        }
        let response = tokio::time::timeout(
            Duration::from_secs(15),
            opds::send_outbound_get(url, reqwest::header::HeaderMap::new(), self.outbound_policy, &format!("Bokheim/{} catalog-cover-client", env!("CARGO_PKG_VERSION")), Duration::from_secs(5), Duration::from_secs(15)),
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

fn cover_reference_key(source_id: &CatalogSourceId, image_url: &str) -> String {
    format!("{}\n{image_url}", source_id.as_str())
}

fn cover_cache_key(image_url: &str) -> String {
    blake3::hash(image_url.as_bytes()).to_hex().to_string()
}

fn cover_path(directory: &Path, image_url: &str) -> PathBuf {
    directory.join(format!("{}.jpg", cover_cache_key(image_url)))
}

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

async fn read_cached_cover(path: &Path) -> Option<Vec<u8>> {
    let metadata = tokio::fs::metadata(path).await.ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_CATALOG_COVER_SOURCE_BYTES {
        return None;
    }
    let bytes = tokio::fs::read(path).await.ok()?;
    let dimensions = ImageReader::new(Cursor::new(bytes.as_slice())).with_guessed_format().ok()?.into_dimensions().ok()?;
    thumbnail::reasonable_cover_dimensions(dimensions.0, dimensions.1).then_some(bytes)
}

#[cfg(test)]
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
}

pub async fn search_all_catalogs(client: &opds::OpdsClient, endpoints: Vec<OpdsEndpoint>, query: &str) -> Result<CatalogWideSearchResult, CatalogProviderError> {
    Ok(CatalogWideSearchResult { query: query.trim().to_owned(), groups: search_opds_catalogs(client, endpoints, query).await? })
}

pub async fn discover_author_books_locally(client: &opds::OpdsClient, endpoints: Vec<OpdsEndpoint>, request: &CatalogAuthorDiscoveryRequest) -> Result<CatalogAuthorDiscoveryResult, CatalogProviderError> {
    validate_author_discovery_request(request)?;
    let identifier_searches = stream::iter(request.book_identifiers.iter().cloned())
        .map(|identifier| {
            let endpoints = endpoints.clone();
            async move { search_all_catalogs(client, endpoints, &identifier.value).await.map(|result| (identifier, result)) }
        })
        .buffer_unordered(request.book_identifiers.len().max(1))
        .collect::<Vec<_>>();
    let name_search = search_all_catalogs(client, endpoints.clone(), &request.author_name);
    let (identifier_searches, name_search) = tokio::join!(identifier_searches, name_search);
    let identifier_searches = identifier_searches.into_iter().collect::<Result<Vec<_>, _>>()?;
    let name_search = name_search?;
    let identifier_searches = identifier_searches.iter().map(|(identifier, result)| (identifier, result)).collect::<Vec<_>>();
    resolve_author_discovery(request, &identifier_searches, &name_search)
}

pub fn resolve_author_discovery(
    request: &CatalogAuthorDiscoveryRequest, identifier_searches: &[(&CatalogPublicationIdentifier, &CatalogWideSearchResult)], name_search: &CatalogWideSearchResult,
) -> Result<CatalogAuthorDiscoveryResult, CatalogProviderError> {
    validate_author_discovery_request(request)?;
    let author_name = AuthorName::parse(&request.author_name).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    let author_key = author_name.match_key().as_str().to_owned();
    let mut resolved = HashMap::<CatalogSourceId, ResolvedCatalogAuthor>::new();

    for &(identifier, search) in identifier_searches {
        if !request.book_identifiers.contains(identifier) {
            continue;
        }
        for group in &search.groups {
            let CatalogWideSearchOutcome::Results { page } = &group.outcome else { continue };
            for publication in page_publications(page).filter(|publication| publication_matches_identifier(publication, identifier)) {
                let matching_authors = publication.authors.iter().filter(|author| author_name_key(&author.name).as_deref() == Some(author_key.as_str())).collect::<Vec<_>>();
                if matching_authors.is_empty() {
                    continue;
                }
                let entry = resolved.entry(group.source.id.clone()).or_default();
                if !entry.identifiers.contains(identifier) {
                    entry.identifiers.push(identifier.clone());
                }
                for author in matching_authors {
                    entry.author_keys.extend(catalog_author_keys(author));
                }
            }
        }
    }

    let mut matches = Vec::<CatalogAuthorDiscoveryMatch>::new();
    let mut match_indices = HashMap::<String, usize>::new();
    for group in &name_search.groups {
        let CatalogWideSearchOutcome::Results { page } = &group.outcome else { continue };
        for publication in page_publications(page) {
            if request.book_identifiers.iter().any(|identifier| publication_matches_identifier(publication, identifier)) {
                continue;
            }
            let matching_authors = publication.authors.iter().filter(|author| author_name_key(&author.name).as_deref() == Some(author_key.as_str())).collect::<Vec<_>>();
            if matching_authors.is_empty() {
                continue;
            }
            let evidence = resolved
                .get(&group.source.id)
                .and_then(|resolved_author| catalog_identifier_evidence(resolved_author, &matching_authors))
                .map(|identifier| CatalogAuthorDiscoveryEvidence::BookIdentifier { identifier })
                .unwrap_or(CatalogAuthorDiscoveryEvidence::AuthorName);
            insert_author_match(&mut matches, &mut match_indices, group.source.clone(), publication.clone(), evidence);
        }
    }

    matches.sort_by_key(|result| match &result.evidence {
        CatalogAuthorDiscoveryEvidence::BookIdentifier { .. } => 0_u8,
        CatalogAuthorDiscoveryEvidence::AuthorName => 1_u8,
    });
    matches.truncate(MAX_AUTHOR_DISCOVERY_MATCHES);
    Ok(CatalogAuthorDiscoveryResult { author_name: author_name.as_str().to_owned(), matches })
}

fn validate_author_discovery_request(request: &CatalogAuthorDiscoveryRequest) -> Result<(), CatalogProviderError> {
    AuthorName::parse(&request.author_name).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    if request.book_identifiers.len() > 8 {
        return Err(CatalogProviderError::new("author discovery accepts at most eight book identifiers"));
    }
    if request.book_identifiers.iter().any(|identifier| !identifier.is_valid_search_identifier()) {
        return Err(CatalogProviderError::new("author discovery contains an invalid book identifier"));
    }
    Ok(())
}

#[derive(Default)]
struct ResolvedCatalogAuthor {
    author_keys: HashSet<String>,
    identifiers: Vec<CatalogPublicationIdentifier>,
}

fn page_publications(page: &CatalogPage) -> impl Iterator<Item = &CatalogPublication> {
    page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter()))
}

fn publication_matches_identifier(publication: &CatalogPublication, identifier: &CatalogPublicationIdentifier) -> bool {
    let Some(expected) = identifier.normalized_value() else { return false };
    [Some(publication.identity.edition_id.as_str()), publication.identity.work_id.as_deref()]
        .into_iter()
        .flatten()
        .any(|value| CatalogPublicationIdentifier { scheme: identifier.scheme, value: value.to_owned() }.normalized_value().as_deref() == Some(expected.as_str()))
}

fn author_name_key(value: &str) -> Option<String> {
    AuthorName::parse(value).ok().map(|name| name.match_key().as_str().to_owned())
}

fn catalog_author_keys(author: &CatalogAuthorCredit) -> HashSet<String> {
    let identifiers = author.identifiers.iter().filter_map(|identifier| {
        let identifier = identifier.trim();
        (!identifier.is_empty()).then(|| format!("identifier:{identifier}"))
    });
    let provider_link = author.provider_link.as_ref().map(|target| match target {
        CatalogLinkTarget::Page { locator } => format!("provider:{locator}"),
        CatalogLinkTarget::External { url } => format!("provider:{url}"),
    });
    identifiers.chain(provider_link).collect()
}

fn catalog_identifier_evidence(resolved_author: &ResolvedCatalogAuthor, matching_authors: &[&CatalogAuthorCredit]) -> Option<CatalogPublicationIdentifier> {
    if resolved_author.author_keys.is_empty() {
        return None;
    }
    let candidate_keys = matching_authors.iter().flat_map(|author| catalog_author_keys(author)).collect::<HashSet<_>>();
    candidate_keys.iter().any(|key| resolved_author.author_keys.contains(key)).then(|| resolved_author.identifiers.first().cloned()).flatten()
}

fn insert_author_match(matches: &mut Vec<CatalogAuthorDiscoveryMatch>, indices: &mut HashMap<String, usize>, source: CatalogSource, publication: CatalogPublication, evidence: CatalogAuthorDiscoveryEvidence) {
    let key = format!("{}\n{}\n{}", source.id.as_str(), publication.identity.edition_id, publication.identity.work_id.as_deref().unwrap_or(""));
    if let Some(index) = indices.get(&key).copied() {
        let existing = &mut matches[index].evidence;
        if matches!(existing, CatalogAuthorDiscoveryEvidence::AuthorName) && matches!(evidence, CatalogAuthorDiscoveryEvidence::BookIdentifier { .. }) {
            *existing = evidence;
        }
        return;
    }
    indices.insert(key, matches.len());
    matches.push(CatalogAuthorDiscoveryMatch { source, publication, evidence });
}

#[doc(hidden)]
pub async fn search_opds_catalogs(client: &opds::OpdsClient, endpoints: Vec<OpdsEndpoint>, query: &str) -> Result<Vec<CatalogWideSearchGroup>, CatalogProviderError> {
    let query = query.trim();
    let search_query = CatalogSearchQuery { query: query.to_owned(), title: None, author: None };
    let terms = search_terms(&search_query)?;
    let plans = endpoints.into_iter().map(|endpoint| (opds_source(&endpoint), endpoint)).collect::<Vec<_>>();
    let concurrency = wide_search_concurrency(plans.len());
    let mut groups = stream::iter(plans.into_iter().enumerate().map(|(index, (source, endpoint))| {
        let terms = terms.clone();
        async move {
            let outcome = search_opds_catalog(client, &source, &endpoint, &terms).await;
            (index, CatalogWideSearchGroup { source, outcome })
        }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<_>>()
    .await;
    groups.sort_by_key(|(index, _)| *index);
    Ok(groups.into_iter().map(|(_, group)| group).collect())
}

#[doc(hidden)]
pub fn wide_search_concurrency(source_count: usize) -> usize {
    source_count.clamp(1, MAX_WIDE_SEARCH_CONCURRENCY)
}

async fn search_opds_catalog(client: &opds::OpdsClient, source: &CatalogSource, endpoint: &OpdsEndpoint, terms: &str) -> CatalogWideSearchOutcome {
    let search = async {
        let root = client.fetch_catalog(&endpoint.url).await.map_err(|error| error.to_string())?;
        validate_opds_catalog_locator(endpoint, &root.source_url).map_err(|error| error.to_string())?;
        let Some(definition) = root.search else { return Ok(None) };
        let search_origin = match &definition {
            OpdsSearch::UriTemplate { base_url, .. } => base_url,
            OpdsSearch::OpenSearchDescription { url } => url,
        };
        validate_opds_catalog_locator(endpoint, search_origin).map_err(|error| error.to_string())?;
        let fields = opds_search(definition.clone()).fields;
        let query = if fields.contains(&CatalogSearchField::Query) {
            OpdsSearchQuery { query: terms.to_owned(), title: None, author: None }
        } else if fields.contains(&CatalogSearchField::Title) {
            OpdsSearchQuery { query: String::new(), title: Some(terms.to_owned()), author: None }
        } else if fields.contains(&CatalogSearchField::Author) {
            OpdsSearchQuery { query: String::new(), title: None, author: Some(terms.to_owned()) }
        } else {
            return Ok(None);
        };
        let catalog = client.search_definition(&definition, &query).await.map_err(|error| error.to_string())?;
        validate_opds_catalog_locator(endpoint, &catalog.source_url).map_err(|error| error.to_string())?;
        Ok::<_, String>(Some(opds_page(source.id.clone(), catalog)))
    };
    match tokio::time::timeout(WIDE_SEARCH_TIMEOUT, search).await {
        Ok(Ok(Some(page))) => CatalogWideSearchOutcome::Results { page },
        Ok(Ok(None)) => CatalogWideSearchOutcome::Unsupported,
        Ok(Err(message)) => CatalogWideSearchOutcome::Failed { message },
        Err(_) => CatalogWideSearchOutcome::TimedOut,
    }
}

pub fn opds_source(endpoint: &OpdsEndpoint) -> CatalogSource {
    CatalogSource {
        id: opds_catalog_source_id(&endpoint.id),
        title: endpoint.title.clone(),
        description: endpoint.description.clone(),
        languages: endpoint.languages.clone(),
        authentication: CatalogAuthentication::None,
        capabilities: vec![CatalogCapability::Browse, CatalogCapability::Search, CatalogCapability::Facets, CatalogCapability::Import, CatalogCapability::ExternalFulfillment],
    }
}

pub fn opds_catalog_source_id(endpoint_id: &OpdsEndpointId) -> CatalogSourceId {
    CatalogSourceId::parse(&format!("{OPDS_SOURCE_PREFIX}{}", endpoint_id.as_str())).expect("namespaced OPDS endpoint id is a catalog source id")
}

#[doc(hidden)]
pub fn opds_endpoint_id(source_id: &CatalogSourceId) -> Result<OpdsEndpointId, CatalogProviderError> {
    let value = source_id.as_str().strip_prefix(OPDS_SOURCE_PREFIX).ok_or_else(|| CatalogProviderError::new("catalog source does not belong to the OPDS provider"))?;
    OpdsEndpointId::parse(value).map_err(|error| CatalogProviderError::new(error.to_string()))
}

pub fn validate_opds_catalog_locator(endpoint: &OpdsEndpoint, locator: &str) -> Result<(), CatalogProviderError> {
    let expected = url::Url::parse(&endpoint.url).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    let actual = url::Url::parse(locator).map_err(|error| CatalogProviderError::new(error.to_string()))?;
    if !matches!(actual.scheme(), "http" | "https") || !actual.username().is_empty() || actual.password().is_some() {
        return Err(CatalogProviderError::new("catalog page locator must be an HTTP URL without embedded credentials"));
    }
    if actual.origin() != expected.origin() {
        return Err(CatalogProviderError::new("catalog page locator does not belong to the registered OPDS origin"));
    }
    Ok(())
}

pub fn opds_page(source_id: CatalogSourceId, catalog: OpdsCatalog) -> CatalogPage {
    let catalog_locator = catalog.source_url.clone();
    CatalogPage {
        source_id: source_id.clone(),
        freshness: match catalog.freshness {
            opds::OpdsCatalogFreshness::Current => CatalogFreshness::Current,
            opds::OpdsCatalogFreshness::Stale => CatalogFreshness::Stale,
        },
        title: catalog.title,
        locator: catalog.source_url,
        navigation: catalog.navigation.into_iter().map(catalog_link).collect(),
        publications: catalog.publications.into_iter().map(|publication| opds_publication(&source_id, &catalog_locator, publication)).collect(),
        groups: catalog.groups.into_iter().map(|group| opds_group(&source_id, &catalog_locator, group)).collect(),
        facets: catalog.facets.into_iter().filter_map(catalog_facet).collect(),
        pagination: CatalogPagination {
            first: catalog.pagination.first,
            previous: catalog.pagination.previous,
            next: catalog.pagination.next,
            last: catalog.pagination.last,
            number_of_items: catalog.pagination.number_of_items,
            items_per_page: catalog.pagination.items_per_page,
            current_page: catalog.pagination.current_page,
        },
        search: catalog.search.map(opds_search),
    }
}

pub fn opds_search_definition(search: &CatalogSearch) -> Result<OpdsSearch, CatalogProviderError> {
    match &search.definition {
        CatalogSearchDefinition::OpdsUriTemplate { base_url, template } => Ok(OpdsSearch::UriTemplate { base_url: base_url.clone(), template: template.clone() }),
        CatalogSearchDefinition::OpdsOpenSearchDescription { url } => Ok(OpdsSearch::OpenSearchDescription { url: url.clone() }),
    }
}

pub fn opds_search_query(query: CatalogSearchQuery) -> OpdsSearchQuery {
    OpdsSearchQuery { query: query.query, title: query.title, author: query.author }
}

/// Revalidates a client-selected catalogue acquisition against a freshly
/// fetched OPDS page. Callers must separately validate that `catalog` belongs
/// to the registered endpoint before invoking this helper.
pub fn validated_opds_import(request: &CatalogImportRequest, catalog: &OpdsCatalog) -> Result<(OpdsPublicationIdentity, OpdsDirectAcquisition), CatalogProviderError> {
    let requested_identity = OpdsPublicationIdentity { publication_id: request.publication.edition_id.clone(), work_id: request.publication.work_id.clone() };
    requested_identity.validate().map_err(|error| CatalogProviderError::new(error.to_string()))?;
    if catalog.source_url != request.catalog_locator {
        return Err(CatalogProviderError::new("catalog import does not belong to the verified catalog page"));
    }
    let publication = catalog
        .publications
        .iter()
        .chain(catalog.groups.iter().flat_map(|group| group.publications.iter()))
        .find(|publication| publication.id == requested_identity.publication_id && publication.work_id == requested_identity.work_id)
        .ok_or_else(|| CatalogProviderError::new("catalog publication is not present on the verified catalog page"))?;
    let requested_format = opds_book_format(request.book_format);
    let acquisition = publication
        .acquisitions
        .iter()
        .find_map(|acquisition| match acquisition {
            OpdsAcquisition::Direct(acquisition)
                if acquisition.url == request.download_url && acquisition.book_format == requested_format && acquisition.price.is_none() && matches!(acquisition.kind, OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic) =>
            {
                Some(acquisition.clone())
            }
            _ => None,
        })
        .ok_or_else(|| CatalogProviderError::new("catalog acquisition is not an importable link on the verified catalog page"))?;
    Ok((requested_identity, acquisition))
}

fn opds_book_format(format: CatalogBookFormat) -> OpdsBookFormat {
    match format {
        CatalogBookFormat::Epub => OpdsBookFormat::Epub,
        CatalogBookFormat::Pdf => OpdsBookFormat::Pdf,
        CatalogBookFormat::Mobi => OpdsBookFormat::Mobi,
        CatalogBookFormat::Fb2 => OpdsBookFormat::Fb2,
        CatalogBookFormat::Cbz => OpdsBookFormat::Cbz,
        CatalogBookFormat::WebPub => OpdsBookFormat::WebPub,
        CatalogBookFormat::M4b => OpdsBookFormat::M4b,
    }
}

#[doc(hidden)]
pub fn search_terms(query: &CatalogSearchQuery) -> Result<String, CatalogProviderError> {
    let terms = [Some(query.query.trim()).filter(|value| !value.is_empty()), query.title.as_deref().map(str::trim).filter(|value| !value.is_empty()), query.author.as_deref().map(str::trim).filter(|value| !value.is_empty())]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if terms.is_empty() || terms.len() > 500 || terms.chars().any(char::is_control) {
        return Err(CatalogProviderError::new("catalog search must contain between 1 and 500 characters without control characters"));
    }
    Ok(terms)
}

fn catalog_link(link: OpdsCatalogLink) -> CatalogLink {
    let target = match link.target {
        OpdsLinkTarget::Catalog => CatalogLinkTarget::Page { locator: link.url },
        OpdsLinkTarget::Web => CatalogLinkTarget::External { url: link.url },
    };
    CatalogLink { title: link.title, target, number_of_items: link.number_of_items, active: link.active }
}

fn catalog_facet(mut facet: OpdsFacet) -> Option<CatalogFacet> {
    if matches!(facet.title.trim().to_ascii_lowercase().as_str(), "license" | "collection") {
        return None;
    }
    if facet.title.trim().eq_ignore_ascii_case("format") {
        facet.links = facet.links.into_iter().filter_map(compact_format_facet_link).collect();
    }
    (!facet.links.is_empty()).then(|| CatalogFacet { title: facet.title, links: facet.links.into_iter().map(catalog_link).collect() })
}

fn compact_format_facet_link(mut link: OpdsCatalogLink) -> Option<OpdsCatalogLink> {
    let words = link.title.split(|character: char| !character.is_ascii_alphanumeric()).filter(|word| !word.is_empty()).collect::<Vec<_>>();
    link.title = if words.iter().any(|word| word.eq_ignore_ascii_case("epub")) {
        "EPUB".to_owned()
    } else if words.iter().any(|word| word.eq_ignore_ascii_case("pdf")) {
        "PDF".to_owned()
    } else if words.iter().any(|word| matches!(word.to_ascii_lowercase().as_str(), "mobi" | "azw" | "azw3")) {
        "MOBI/AZW".to_owned()
    } else if words.iter().any(|word| word.eq_ignore_ascii_case("fb2")) {
        "FB2".to_owned()
    } else if words.iter().any(|word| word.eq_ignore_ascii_case("cbz")) {
        "CBZ".to_owned()
    } else if words.iter().any(|word| word.eq_ignore_ascii_case("webpub")) {
        "WebPub".to_owned()
    } else if words.iter().any(|word| word.eq_ignore_ascii_case("m4b")) {
        "M4B".to_owned()
    } else {
        return None;
    };
    Some(link)
}

fn opds_group(source_id: &CatalogSourceId, catalog_locator: &str, group: OpdsGroup) -> CatalogGroup {
    CatalogGroup {
        title: group.title,
        page_locator: group.self_url,
        number_of_items: group.number_of_items,
        navigation: group.navigation.into_iter().map(catalog_link).collect(),
        publications: group.publications.into_iter().map(|publication| opds_publication(source_id, catalog_locator, publication)).collect(),
    }
}

#[doc(hidden)]
pub fn opds_publication(source_id: &CatalogSourceId, catalog_locator: &str, publication: OpdsPublication) -> CatalogPublication {
    let identity = CatalogPublicationIdentity { edition_id: publication.id, work_id: publication.work_id };
    let mut import_formats = Vec::new();
    let actions = publication
        .acquisitions
        .into_iter()
        .filter_map(|acquisition| opds_action(source_id, catalog_locator, &identity, acquisition))
        .filter(|action| match action {
            CatalogPublicationAction::Import { request } if import_formats.contains(&request.book_format) => false,
            CatalogPublicationAction::Import { request } => {
                import_formats.push(request.book_format);
                true
            }
            CatalogPublicationAction::External { .. } => true,
        })
        .collect();
    let description = publication.description.map(|description| match description {
        OpdsDescription::PlainText { text } => CatalogDescription::PlainText { text },
        OpdsDescription::Html { safe_html, plain_text, .. } | OpdsDescription::Xhtml { safe_html, plain_text, .. } => CatalogDescription::RichHtml { safe_html, plain_text },
    });
    let authors = publication
        .authors
        .into_iter()
        .map(|author| CatalogAuthorCredit {
            name: author.name,
            sort_name: author.sort_name,
            identifiers: author.identifiers,
            provider_link: author.provider_link.map(|link| match link.target {
                OpdsLinkTarget::Catalog => CatalogLinkTarget::Page { locator: link.url },
                OpdsLinkTarget::Web => CatalogLinkTarget::External { url: link.url },
            }),
        })
        .collect();
    CatalogPublication { identity, title: publication.title, authors, languages: publication.languages, description, image_url: publication.image_url, actions }
}

fn opds_action(source_id: &CatalogSourceId, catalog_locator: &str, identity: &CatalogPublicationIdentity, acquisition: OpdsAcquisition) -> Option<CatalogPublicationAction> {
    match acquisition {
        OpdsAcquisition::Direct(acquisition) if acquisition.price.is_none() && matches!(acquisition.kind, OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic) => Some(CatalogPublicationAction::Import {
            request: CatalogImportRequest { source_id: source_id.clone(), catalog_locator: catalog_locator.to_owned(), publication: identity.clone(), download_url: acquisition.url, book_format: book_format(acquisition.book_format) },
        }),
        OpdsAcquisition::Direct(acquisition) => {
            Some(CatalogPublicationAction::External { purpose: external_purpose(acquisition.kind), url: acquisition.url, price: acquisition.price.map(price), available_formats: vec![book_format(acquisition.book_format)] })
        }
        OpdsAcquisition::Indirect(acquisition) => Some(CatalogPublicationAction::External {
            purpose: external_purpose(acquisition.kind),
            url: acquisition.url,
            price: acquisition.price.map(price),
            available_formats: acquisition.available_formats.into_iter().map(book_format).collect(),
        }),
        OpdsAcquisition::Unsupported(acquisition) => Some(CatalogPublicationAction::External { purpose: external_purpose(acquisition.kind), url: acquisition.url, price: acquisition.price.map(price), available_formats: Vec::new() }),
    }
}

#[doc(hidden)]
pub fn opds_search(search: OpdsSearch) -> CatalogSearch {
    let mut fields = vec![CatalogSearchField::Query];
    if let OpdsSearch::UriTemplate { template, .. } = &search {
        if template_has_variable(template, "title") {
            fields.push(CatalogSearchField::Title);
        }
        if template_has_variable(template, "author") {
            fields.push(CatalogSearchField::Author);
        }
    }
    let definition = match search {
        OpdsSearch::UriTemplate { base_url, template } => CatalogSearchDefinition::OpdsUriTemplate { base_url, template },
        OpdsSearch::OpenSearchDescription { url } => CatalogSearchDefinition::OpdsOpenSearchDescription { url },
    };
    CatalogSearch { definition, fields }
}

fn template_has_variable(template: &str, expected: &str) -> bool {
    template
        .split('{')
        .skip(1)
        .filter_map(|suffix| suffix.split_once('}').map(|(expression, _)| expression))
        .flat_map(|expression| expression.trim_start_matches(['+', '#', '.', '/', ';', '?', '&']).split(','))
        .any(|variable| variable.trim_end_matches(['?', '*']).split(':').next() == Some(expected))
}

fn book_format(format: OpdsBookFormat) -> CatalogBookFormat {
    match format {
        OpdsBookFormat::Epub => CatalogBookFormat::Epub,
        OpdsBookFormat::Pdf => CatalogBookFormat::Pdf,
        OpdsBookFormat::Mobi => CatalogBookFormat::Mobi,
        OpdsBookFormat::Fb2 => CatalogBookFormat::Fb2,
        OpdsBookFormat::Cbz => CatalogBookFormat::Cbz,
        OpdsBookFormat::WebPub => CatalogBookFormat::WebPub,
        OpdsBookFormat::M4b => CatalogBookFormat::M4b,
    }
}

fn price(value: OpdsPrice) -> CatalogPrice {
    CatalogPrice { currency: value.currency, value: value.value }
}

fn external_purpose(kind: OpdsAcquisitionKind) -> CatalogExternalActionKind {
    match kind {
        OpdsAcquisitionKind::Preview => CatalogExternalActionKind::Preview,
        OpdsAcquisitionKind::Buy => CatalogExternalActionKind::Buy,
        OpdsAcquisitionKind::Borrow => CatalogExternalActionKind::Borrow,
        OpdsAcquisitionKind::Subscribe => CatalogExternalActionKind::Subscribe,
        OpdsAcquisitionKind::OpenAccess | OpdsAcquisitionKind::Generic => CatalogExternalActionKind::Open,
    }
}
