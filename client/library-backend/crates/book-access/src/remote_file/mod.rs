//! Format-independent, seekable remote file access. Cached fragments are never
//! installed as a downloaded asset or treated as a verified complete file.

use crate::BookId;
use std::fmt::Formatter;
use std::time::Duration;

pub(crate) use crate::range_cache::MAX_BUNDLE_BYTES;
use crate::range_reader::{BLOCK_BYTES, MAX_RANGE_BYTES};
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(not(target_arch = "wasm32"))]
mod audiobook_stream;
#[cfg(not(target_arch = "wasm32"))]
pub use audiobook_stream::native_audio_reader;
#[cfg(not(target_arch = "wasm32"))]
pub use audiobook_stream::AudioStreamControl;

/// File identity and length, with no URL, credentials, or format knowledge.
/// The owning library resolves and authenticates byte reads.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteFile {
    pub hash: BookId,
    pub length: u64,
}

impl std::fmt::Debug for RemoteFile {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteFile").field("length", &self.length).field("hash", &self.hash).finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum RemoteFileError {
    Unauthorized,
    Retryable(String),
    Failed(String),
}
impl std::fmt::Display for RemoteFileError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => f.write_str("unauthorized"),
            Self::Retryable(error) | Self::Failed(error) => f.write_str(error),
        }
    }
}
impl std::error::Error for RemoteFileError {}

impl From<String> for RemoteFileError {
    fn from(error: String) -> Self {
        Self::Failed(error)
    }
}
impl From<&str> for RemoteFileError {
    fn from(error: &str) -> Self {
        Self::Failed(error.into())
    }
}

impl RemoteFileError {
    pub fn transport(error: reqwest::Error) -> Self {
        // URL removal also prevents signed URLs or access tokens reaching logs.
        Self::Retryable(error.without_url().to_string())
    }

    pub fn status(status: reqwest::StatusCode) -> Self {
        let message = format!("remote file range request returned HTTP {status}");
        if status == reqwest::StatusCode::UNAUTHORIZED {
            Self::Unauthorized
        } else if status.is_server_error() || status == reqwest::StatusCode::REQUEST_TIMEOUT || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            Self::Retryable(message)
        } else {
            Self::Failed(message)
        }
    }
}

/// Open with a bounded prefix read so discovering the length also fills the cache.
pub async fn describe(client: &reqwest::Client, url: reqwest::Url, access_token: &str, hash: BookId) -> Result<(RemoteFile, Vec<u8>), RemoteFileError> {
    let request = async {
        let response = request_range(client, url, access_token, hash, 0, BLOCK_BYTES, false).await?;
        let length = response.headers().get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok()).and_then(|v| v.rsplit_once('/')).and_then(|(_, total)| total.parse::<u64>().ok()).filter(|&n| n > 0 && n <= 9_007_199_254_740_991);
        let Some(length) = length else {
            drop(response.bytes_stream());
            return Err("invalid remote file length".into());
        };
        let source = RemoteFile { hash, length };
        let bytes = read_range_response(response, &source, 0, length.min(BLOCK_BYTES as u64) as usize).await?;
        Ok((source, bytes))
    };
    crate::time::timeout(REQUEST_TIMEOUT, request).await.map_err(|_| RemoteFileError::Retryable("remote file opening request timed out".to_owned()))?
}

/// One request path for prefix discovery and revision-pinned subsequent reads.
async fn request_range(client: &reqwest::Client, url: reqwest::Url, token: &str, hash: BookId, offset: u64, length: usize, pin_revision: bool) -> Result<reqwest::Response, RemoteFileError> {
    let end = offset + length as u64 - 1;
    let etag = format!("\"{hash}\"");
    let mut request = client.get(url).bearer_auth(token).header(reqwest::header::RANGE, format!("bytes={offset}-{end}"));
    if pin_revision {
        request = request.header(reqwest::header::IF_RANGE, &etag);
    }
    let response = request.send().await.map_err(RemoteFileError::transport)?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        let error = RemoteFileError::status(response.status());
        drop(response.bytes_stream());
        return Err(error);
    }
    if response.headers().get(reqwest::header::ETAG).and_then(|v| v.to_str().ok()) != Some(etag.as_str()) {
        drop(response.bytes_stream());
        return Err("remote file revision changed".into());
    }
    Ok(response)
}

pub async fn fetch_range(client: &reqwest::Client, url: reqwest::Url, source: &RemoteFile, token: &str, offset: u64, length: usize) -> Result<Vec<u8>, RemoteFileError> {
    if length == 0 || length > MAX_RANGE_BYTES || offset.checked_add(length as u64).is_none_or(|end| end > source.length) {
        return Err("invalid remote file range".into());
    }
    let response = request_range(client, url, token, source.hash, offset, length, true).await?;
    read_range_response(response, source, offset, length).await
}

async fn read_range_response(response: reqwest::Response, source: &RemoteFile, offset: u64, length: usize) -> Result<Vec<u8>, RemoteFileError> {
    let end = offset + length as u64 - 1;
    let expected = format!("bytes {offset}-{end}/{}", source.length);
    if response.headers().get(reqwest::header::CONTENT_RANGE).and_then(|v| v.to_str().ok()) != Some(expected.as_str()) {
        drop(response.bytes_stream());
        return Err("remote file range bounds changed".into());
    }
    read_bounded_response(response, length).await
}

async fn read_bounded_response(response: reqwest::Response, length: usize) -> Result<Vec<u8>, RemoteFileError> {
    use futures_util::StreamExt;
    let mut bytes = Vec::with_capacity(length);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(RemoteFileError::transport)?;
        if chunk.len() > length - bytes.len() {
            return Err("oversized remote file range".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() != length {
        return Err(RemoteFileError::Retryable("incomplete remote file range".into()));
    }
    Ok(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::native_reader;

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
pub use browser::{browser_bundle_fetch, browser_reader, BrowserReader};

/// Validate local bundle bounds before issuing any network requests.
pub(crate) fn bundle_byte_count(ranges: &[(u64, usize)], length: u64) -> Option<usize> {
    if ranges.is_empty() || ranges.len() > 128 {
        return None;
    }
    let mut end = 0;
    let mut total = 0usize;
    for &(offset, count) in ranges {
        if count == 0 || offset < end {
            return None;
        }
        end = offset.checked_add(count as u64)?;
        total = total.checked_add(count)?;
        if end > length || total > MAX_BUNDLE_BYTES {
            return None;
        }
    }
    Some(total)
}

/// Fetch bounded HTTP ranges concurrently, concatenating them in request order.
pub async fn fetch_bundle(client: &reqwest::Client, url: reqwest::Url, source: &RemoteFile, token: &str, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, RemoteFileError> {
    use futures_util::{StreamExt, TryStreamExt};
    let length = bundle_byte_count(&ranges, source.length).ok_or("invalid book bundle")?;
    let chunks = ranges.into_iter().flat_map(|(offset, count)| (0..count).step_by(MAX_RANGE_BYTES).map(move |start| (offset + start as u64, (count - start).min(MAX_RANGE_BYTES))));
    let mut responses = futures_util::stream::iter(chunks.map(|(offset, count)| fetch_range(client, url.clone(), source, token, offset, count))).buffered(8);
    let mut bytes = Vec::with_capacity(length);
    while let Some(chunk) = responses.try_next().await? {
        bytes.extend(chunk);
    }
    Ok(bytes)
}
