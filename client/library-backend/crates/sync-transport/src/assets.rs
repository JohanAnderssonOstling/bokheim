use crate::PerformanceTrace;
use crate::api::{send, SyncCredentials};
use crate::state::SyncRequestError;
use std::{error::Error, fmt};
use sync_common::api::assets::{
    validate_blob_manifest_response, BlobManifestEntry, BlobManifestRequest, BlobManifestResponse, ThumbnailBatchDownloadRequest, ThumbnailBatchDownloadResponse, ThumbnailBatchEntry, ThumbnailBatchUploadRequest, ThumbnailPresenceRequest,
    ThumbnailPresenceResponse,
};
use sync_common::{ContentHash, LibraryId};

/// HTTP and wire-format failures during asset transfers.
///
/// Clients keep ownership of file IO and downloads, while
/// this crate owns the sync service's HTTP status semantics.
#[derive(Debug)]
pub enum AssetResponseError {
    AuthenticationRequired,
    StorageQuotaExceeded,
    Retryable(reqwest::Error),
    Rejected(reqwest::Error),
    Wire(sync_common::transport::WireError),
}

impl fmt::Display for AssetResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str("authentication access token was rejected"),
            Self::StorageQuotaExceeded => formatter.write_str("book storage quota exceeded"),
            Self::Retryable(error) | Self::Rejected(error) => error.fmt(formatter),
            Self::Wire(error) => error.fmt(formatter),
        }
    }
}

impl Error for AssetResponseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Retryable(error) | Self::Rejected(error) => Some(error),
            Self::Wire(error) => Some(error),
            Self::AuthenticationRequired | Self::StorageQuotaExceeded => None,
        }
    }
}

/// Applies the sync service's common status rules while preserving the body
/// for a streaming native client.
pub fn validate_streaming_asset_response(response: reqwest::Response) -> Result<reqwest::Response, AssetResponseError> {
    match classify_asset_status(response.status().as_u16()) {
        AssetStatus::Success => Ok(response),
        AssetStatus::AuthenticationRequired => Err(AssetResponseError::AuthenticationRequired),
        AssetStatus::StorageQuotaExceeded => Err(AssetResponseError::StorageQuotaExceeded),
        AssetStatus::Rejected => Err(AssetResponseError::Rejected(response.error_for_status().unwrap_err())),
        AssetStatus::Retryable => Err(AssetResponseError::Retryable(response.error_for_status().unwrap_err())),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AssetStatus {
    Success,
    AuthenticationRequired,
    StorageQuotaExceeded,
    Retryable,
    Rejected,
}

/// Shared HTTP policy for reqwest responses and browser File uploads.
pub fn classify_asset_status(status: u16) -> AssetStatus {
    match status {
        401 => AssetStatus::AuthenticationRequired,
        507 => AssetStatus::StorageQuotaExceeded,
        0 | 408 | 429 | 500..=599 => AssetStatus::Retryable,
        400..=499 => AssetStatus::Rejected,
        _ => AssetStatus::Success,
    }
}

pub fn blob_endpoint(server_url: &reqwest::Url, library_id: &LibraryId, content_hash: &ContentHash) -> reqwest::Url {
    server_url.join(&format!("api/libraries/{library_id}/blobs/{content_hash}")).expect("a synchronization server origin accepts relative API paths")
}

pub fn thumbnail_batch_endpoint(server_url: &reqwest::Url, library_id: &LibraryId) -> reqwest::Url {
    server_url.join(&format!("api/libraries/{library_id}/thumbnail-batch")).expect("a synchronization server origin accepts relative API paths")
}
pub fn thumbnail_presence_endpoint(server_url: &reqwest::Url, library_id: &LibraryId) -> reqwest::Url {
    server_url.join(&format!("api/libraries/{library_id}/thumbnails/presence")).expect("valid endpoint")
}
pub fn thumbnail_endpoint(server_url: &reqwest::Url, library_id: &LibraryId, content_hash: &ContentHash) -> reqwest::Url {
    server_url.join(&format!("api/libraries/{library_id}/thumbnails/{content_hash}")).expect("valid endpoint")
}

pub async fn thumbnail_presence(client: &reqwest::Client, credentials: &SyncCredentials, library_id: &LibraryId, content_hashes: Vec<ContentHash>) -> Result<ThumbnailPresenceResponse, SyncRequestError> {
    send(client.post(credentials.server_url().join(&format!("api/libraries/{library_id}/thumbnails/presence"))?).bearer_auth(credentials.access_token()), &ThumbnailPresenceRequest { content_hashes }).await
}

pub async fn negotiate_blobs(client: &reqwest::Client, credentials: &SyncCredentials, library_id: &LibraryId, blobs: Vec<BlobManifestEntry>) -> Result<BlobManifestResponse, SyncRequestError> {
    let request = BlobManifestRequest { blobs };
    let response: BlobManifestResponse = send(client.post(credentials.server_url().join(&format!("api/libraries/{library_id}/blobs/negotiate"))?).bearer_auth(credentials.access_token()), &request).await?;
    validate_blob_manifest_response(&request, &response).map_err(SyncRequestError::InvalidManifest)?;
    Ok(response)
}

/// Upload bytes addressed and verified by their content hash.
pub async fn upload_book_stream(
    client: &reqwest::Client, server_url: &reqwest::Url, access_token: &str, library_id: &LibraryId, identity: &ContentHash, content_length: u64, body: reqwest::Body,
) -> Result<(), AssetResponseError> {
    let mut trace = PerformanceTrace::new("book_http_upload", "send_body_wait_response");
    log::debug!(target: "sync_performance", "trace_id={} library_id={} bytes={content_length}", trace.id(), library_id);
    let upload_started = web_time::Instant::now();
    let response = client
        .put(blob_endpoint(server_url, library_id, identity))
        .bearer_auth(access_token)
        .header("x-bokheim-trace-id", trace.id())
        .header(reqwest::header::CONTENT_LENGTH, content_length)
        .body(body)
        .send()
        .await
        .map_err(AssetResponseError::Retryable)?;
    let upload_ms = upload_started.elapsed().as_secs_f64() * 1000.0;
    log::debug!(target: "sync_performance", "trace_id={} http_status={}", trace.id(), response.status().as_u16());
    let result = validate_streaming_asset_response(response);
    let successful_book_bytes = if result.is_ok() { content_length } else { 0 };
    log::debug!(target: "sync_performance", "side=client trace_id={} event=book_upload_sample mode=individual book_bytes={content_length} successful_book_bytes={successful_book_bytes} upload_ms={upload_ms:.3}", trace.id());
    trace.finish(result.is_ok());
    result?;
    Ok(())
}

pub async fn upload_thumbnail_batch(client: &reqwest::Client, server_url: &reqwest::Url, access_token: &str, library_id: &LibraryId, thumbnails: Vec<ThumbnailBatchEntry>) -> Result<(), AssetResponseError> {
    let mut trace = PerformanceTrace::new("thumbnail_http_upload", "encode");
    log::debug!(target: "sync_performance", "trace_id={} library_id={} thumbnails={}", trace.id(), library_id, thumbnails.len());
    let body = sync_common::transport::encode(&ThumbnailBatchUploadRequest { thumbnails }).map_err(AssetResponseError::Wire)?;
    trace.phase("send_body_wait_response");
    let response = client
        .put(thumbnail_batch_endpoint(server_url, library_id))
        .header("x-bokheim-trace-id", trace.id())
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, sync_common::transport::MEDIA_TYPE)
        .header(reqwest::header::CONTENT_TYPE, sync_common::transport::MEDIA_TYPE)
        .body(body)
        .send()
        .await
        .map_err(AssetResponseError::Retryable)?;
    log::debug!(target: "sync_performance", "trace_id={} http_status={}", trace.id(), response.status().as_u16());
    let result = validate_streaming_asset_response(response);
    trace.finish(result.is_ok());
    result?;
    Ok(())
}

pub async fn download_thumbnail_batch(client: &reqwest::Client, server_url: &reqwest::Url, access_token: &str, library_id: &LibraryId, content_hashes: Vec<ContentHash>) -> Result<ThumbnailBatchDownloadResponse, AssetResponseError> {
    let body = sync_common::transport::encode(&ThumbnailBatchDownloadRequest { content_hashes }).map_err(AssetResponseError::Wire)?;
    let response = client
        .post(thumbnail_batch_endpoint(server_url, library_id))
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, sync_common::transport::MEDIA_TYPE)
        .header(reqwest::header::CONTENT_TYPE, sync_common::transport::MEDIA_TYPE)
        .body(body)
        .send()
        .await
        .map_err(AssetResponseError::Retryable)?;
    let response = validate_streaming_asset_response(response)?;
    let bytes = response.bytes().await.map_err(AssetResponseError::Retryable)?;
    sync_common::transport::decode(&bytes, sync_common::api::assets::MAX_THUMBNAIL_BATCH_DOWNLOAD_BYTES).map_err(AssetResponseError::Wire)
}
