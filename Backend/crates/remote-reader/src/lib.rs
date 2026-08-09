//! Provides sparse, range-backed reading and bounded on-disk caching of remote books.

use bokheim_identifiers::{ContentHash, LibraryId};
use bokheim_sync_client::ServerUrl;
use reqwest::blocking::Client;
use reqwest::header::{CONTENT_LENGTH, ETAG, IF_RANGE, RANGE};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

const FETCH_CHUNK_BYTES: u64 = 256 * 1024;
static FETCH_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();
#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
const CACHE_BUDGET_BYTES: u64 = 256 * 1024 * 1024;
#[cfg(not(any(target_arch = "arm", target_arch = "aarch64")))]
const CACHE_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct SparseCacheMetadata {
    total_len: u64,
    etag: String,
    ranges: Vec<(u64, u64)>,
}

pub struct RemoteAssetReader {
    client: Client,
    endpoint: reqwest::Url,
    token: String,
    content_hash: ContentHash,
    data_path: PathBuf,
    metadata_path: PathBuf,
    metadata: SparseCacheMetadata,
    file: File,
    position: u64,
    fetch_lock: Arc<Mutex<()>>,
}

impl RemoteAssetReader {
    pub fn open(cache_root: &Path, account_id: &str, library_id: &LibraryId, content_hash: ContentHash, server_url: &ServerUrl, token: String) -> io::Result<Self> {
        let directory = cache_root.join(account_id).join(library_id.as_str()).join(content_hash.as_str());
        enforce_cache_budget(&cache_root.join(account_id), CACHE_BUDGET_BYTES, &directory);
        fs::create_dir_all(&directory)?;
        let data_path = directory.join("archive.sparse");
        let metadata_path = directory.join("ranges.json");
        let cached = fs::read(&metadata_path).ok().and_then(|bytes| serde_json::from_slice::<SparseCacheMetadata>(&bytes).ok());
        let client = Client::builder().timeout(Duration::from_secs(30)).build().map_err(io::Error::other)?;
        let endpoint = server_url.endpoint(&format!("api/libraries/{}/blobs/{}", urlencoding::encode(library_id.as_str()), urlencoding::encode(content_hash.as_str())));
        let expected_etag = format!("\"{}\"", content_hash.as_str());
        let remote_metadata = client.head(endpoint.clone()).bearer_auth(&token).send().ok().filter(|response| response.status().is_success()).and_then(|response| {
            let total_len = response.headers().get(CONTENT_LENGTH)?.to_str().ok()?.parse().ok()?;
            let etag = response.headers().get(ETAG)?.to_str().ok()?.to_owned();
            (etag == expected_etag).then_some(SparseCacheMetadata { total_len, etag, ranges: Vec::new() })
        });
        let metadata = match (remote_metadata, cached) {
            (Some(mut remote), Some(cached)) if remote.total_len == cached.total_len && remote.etag == cached.etag => {
                remote.ranges = cached.ranges;
                remote
            }
            (Some(remote), _) => remote,
            (None, Some(cached)) if cached.etag == expected_etag => cached,
            (None, _) => return Err(io::Error::new(io::ErrorKind::NotConnected, "book is unavailable and has no cached sections")),
        };
        let file = OpenOptions::new().create(true).read(true).write(true).open(&data_path)?;
        file.set_len(metadata.total_len)?;
        let fetch_lock = cache_fetch_lock(&data_path);
        Ok(Self { client, endpoint, token, content_hash, data_path, metadata_path, metadata, file, position: 0, fetch_lock })
    }

    pub fn len(&self) -> u64 {
        self.metadata.total_len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn contains(&self, start: u64, end: u64) -> bool {
        self.metadata.ranges.iter().any(|&(cached_start, cached_end)| cached_start <= start && cached_end >= end)
    }

    fn ensure_cached(&mut self, start: u64, end: u64) -> io::Result<()> {
        if start >= end || self.contains(start, end) {
            return Ok(());
        }
        let fetch_lock = Arc::clone(&self.fetch_lock);
        let _guard = fetch_lock.lock().map_err(|_| io::Error::other("remote cache fetch lock is poisoned"))?;
        self.refresh_ranges();
        while !self.contains(start, end) {
            let (fetch_start, fetch_end) = self.next_missing_range(start, end).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "remote cache could not determine a missing range"))?;
            self.fetch_range(fetch_start, fetch_end)?;
        }
        Ok(())
    }

    fn next_missing_range(&self, start: u64, end: u64) -> Option<(u64, u64)> {
        let mut missing_start = start;
        for &(cached_start, cached_end) in &self.metadata.ranges {
            if cached_start <= missing_start && cached_end > missing_start {
                missing_start = cached_end;
            }
        }
        let mut fetch_start = (missing_start / FETCH_CHUNK_BYTES) * FETCH_CHUNK_BYTES;
        for &(cached_start, cached_end) in &self.metadata.ranges {
            if cached_start <= fetch_start && cached_end > fetch_start {
                fetch_start = cached_end;
            }
        }
        let minimum_end = fetch_start.saturating_add(FETCH_CHUNK_BYTES).max(end);
        let mut fetch_end = minimum_end.saturating_add(FETCH_CHUNK_BYTES - 1) / FETCH_CHUNK_BYTES * FETCH_CHUNK_BYTES;
        fetch_end = fetch_end.min(self.metadata.total_len);
        if let Some(cached_start) = self.metadata.ranges.iter().map(|range| range.0).filter(|cached_start| *cached_start > fetch_start && *cached_start < fetch_end).min() {
            fetch_end = cached_start;
        }
        (fetch_start < fetch_end).then_some((fetch_start, fetch_end))
    }

    fn fetch_range(&mut self, start: u64, end: u64) -> io::Result<()> {
        let response = self
            .client
            .get(self.endpoint.clone())
            .bearer_auth(&self.token)
            .header(RANGE, format!("bytes={start}-{}", end - 1))
            .header(IF_RANGE, &self.metadata.etag)
            .send()
            .map_err(|error| io::Error::new(io::ErrorKind::NotConnected, error))?;
        if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("server rejected asset range request with {}", response.status())));
        }
        if response.headers().get(ETAG).and_then(|value| value.to_str().ok()) != Some(self.metadata.etag.as_str()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "server asset ETag changed during reading"));
        }
        let bytes = response.bytes().map_err(io::Error::other)?;
        if bytes.len() as u64 != end - start {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "server returned an incomplete publication range"));
        }
        self.file.seek(SeekFrom::Start(start))?;
        self.file.write_all(&bytes)?;
        self.file.flush()?;
        self.metadata.ranges.push((start, end));
        self.merge_ranges();
        self.persist_metadata()
    }

    fn refresh_ranges(&mut self) {
        let Some(metadata) = fs::read(&self.metadata_path).ok().and_then(|bytes| serde_json::from_slice::<SparseCacheMetadata>(&bytes).ok()) else { return };
        if metadata.total_len == self.metadata.total_len && metadata.etag == self.metadata.etag {
            self.metadata.ranges = metadata.ranges;
        }
    }

    fn merge_ranges(&mut self) {
        self.metadata.ranges.sort_unstable();
        let mut merged: Vec<(u64, u64)> = Vec::with_capacity(self.metadata.ranges.len());
        for (start, end) in self.metadata.ranges.drain(..) {
            if let Some(last) = merged.last_mut() {
                if start <= last.1 {
                    last.1 = last.1.max(end);
                    continue;
                }
            }
            merged.push((start, end));
        }
        self.metadata.ranges = merged;
    }

    fn persist_metadata(&self) -> io::Result<()> {
        let bytes = serde_json::to_vec(&self.metadata).map_err(io::Error::other)?;
        let temporary = self.metadata_path.with_extension("json.tmp");
        fs::write(&temporary, bytes)?;
        fs::rename(temporary, &self.metadata_path)
    }
}

fn cache_fetch_lock(data_path: &Path) -> Arc<Mutex<()>> {
    let locks = FETCH_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks.lock().expect("remote cache lock registry is poisoned");
    if let Some(lock) = locks.get(data_path).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(data_path.to_owned(), Arc::downgrade(&lock));
    lock
}

fn enforce_cache_budget(account_root: &Path, budget: u64, preserve: &Path) {
    let mut caches = Vec::new();
    let Ok(libraries) = fs::read_dir(account_root) else { return };
    for library in libraries.flatten() {
        let Ok(books) = fs::read_dir(library.path()) else { continue };
        for book in books.flatten() {
            let path = book.path();
            if path == preserve {
                continue;
            }
            let metadata_path = path.join("ranges.json");
            let Some(metadata) = fs::read(&metadata_path).ok().and_then(|bytes| serde_json::from_slice::<SparseCacheMetadata>(&bytes).ok()) else { continue };
            let used = metadata.ranges.iter().map(|(start, end)| end.saturating_sub(*start)).sum::<u64>();
            let modified = fs::metadata(&metadata_path).and_then(|metadata| metadata.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            caches.push((modified, used, path));
        }
    }
    let mut total = caches.iter().map(|(_, used, _)| *used).sum::<u64>();
    caches.sort_by_key(|(modified, _, _)| *modified);
    for (_, used, path) in caches {
        if total <= budget {
            break;
        }
        if fs::remove_dir_all(path).is_ok() {
            total = total.saturating_sub(used);
        }
    }
}

impl Read for RemoteAssetReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.position >= self.metadata.total_len {
            return Ok(0);
        }
        let end = self.position.saturating_add(buffer.len() as u64).min(self.metadata.total_len);
        self.ensure_cached(self.position, end)?;
        self.file.seek(SeekFrom::Start(self.position))?;
        let available = usize::try_from(end - self.position).unwrap_or(buffer.len()).min(buffer.len());
        let count = self.file.read(&mut buffer[..available])?;
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for RemoteAssetReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let next = match position {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::End(value) => i128::from(self.metadata.total_len) + i128::from(value),
            SeekFrom::Current(value) => i128::from(self.position) + i128::from(value),
        };
        if next < 0 || next > i128::from(self.metadata.total_len) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek outside remote asset"));
        }
        self.position = next as u64;
        Ok(self.position)
    }
}

impl std::fmt::Debug for RemoteAssetReader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("RemoteAssetReader").field("content_hash", &self.content_hash).field("data_path", &self.data_path).field("position", &self.position).finish()
    }
}

pub fn copy_cached_prefix(cache_root: &Path, account_id: &str, library_id: &LibraryId, content_hash: &ContentHash, destination: &Path) -> io::Result<u64> {
    let directory = cache_root.join(account_id).join(library_id.as_str()).join(content_hash.as_str());
    let metadata: SparseCacheMetadata = match fs::read(directory.join("ranges.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let prefix_len = metadata.ranges.iter().find_map(|&(start, end)| (start == 0).then_some(end)).unwrap_or(0);
    if prefix_len == 0 {
        return Ok(0);
    }
    let existing_len = fs::metadata(destination).map(|metadata| metadata.len()).unwrap_or(0);
    if existing_len >= prefix_len {
        return Ok(existing_len);
    }
    let source = File::open(directory.join("archive.sparse"))?;
    let mut destination = OpenOptions::new().create(true).write(true).truncate(true).open(destination)?;
    io::copy(&mut source.take(prefix_len), &mut destination)?;
    destination.flush()?;
    Ok(prefix_len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::{TcpListener, TcpStream};

    fn request_headers(stream: &mut TcpStream) -> Vec<String> {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut headers = Vec::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
            headers.push(line.trim().to_owned());
        }
        headers
    }

    #[test]
    fn fetches_only_requested_range_and_reopens_cached_range_offline() {
        const TOTAL: u64 = 1024 * 1024;
        let hash = ContentHash::parse(&"a".repeat(64)).unwrap();
        let etag = format!("\"{}\"", hash.as_str());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for request_index in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let headers = request_headers(&mut stream);
                if request_index == 0 {
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {TOTAL}\r\nETag: {etag}\r\nConnection: close\r\n\r\n").unwrap();
                } else {
                    let range = headers.iter().find_map(|header| header.to_ascii_lowercase().strip_prefix("range: bytes=").map(str::to_owned)).unwrap();
                    let (start, end) = range.split_once('-').unwrap();
                    let start: u64 = start.parse().unwrap();
                    let end: u64 = end.parse().unwrap();
                    let bytes = (start..=end).map(|offset| (offset % 251) as u8).collect::<Vec<_>>();
                    write!(stream, "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nETag: {etag}\r\nConnection: close\r\n\r\n", bytes.len()).unwrap();
                    stream.write_all(&bytes).unwrap();
                }
            }
        });

        let cache = tempfile::tempdir().unwrap();
        let library = LibraryId::parse("library").unwrap();
        let server_url = ServerUrl::parse(&format!("http://{address}")).unwrap();
        let mut reader = RemoteAssetReader::open(cache.path(), "account", &library, hash.clone(), &server_url, "token".to_owned()).unwrap();
        reader.seek(SeekFrom::Start(700_000)).unwrap();
        let mut bytes = [0_u8; 4];
        reader.read_exact(&mut bytes).unwrap();
        drop(reader);
        server.join().unwrap();

        let mut cached = RemoteAssetReader::open(cache.path(), "account", &library, hash, &server_url, "token".to_owned()).unwrap();
        cached.seek(SeekFrom::Start(700_000)).unwrap();
        let mut cached_bytes = [0_u8; 4];
        cached.read_exact(&mut cached_bytes).unwrap();
        assert_eq!(cached_bytes, bytes);
        cached.seek(SeekFrom::Start(100_000)).unwrap();
        assert_eq!(cached.read(&mut cached_bytes).unwrap_err().kind(), io::ErrorKind::NotConnected);
    }
}
