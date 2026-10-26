use crate::PerformanceTrace;
use crate::api::{encoded_request, send, SyncCredentials};
use crate::state::SyncRequestError;
use std::{error::Error, fmt};
use sync_common::api::assets::{
    validate_blob_manifest_response, BlobManifestEntry, BlobManifestRequest, BlobManifestResponse, ThumbnailBatchDownloadRequest, ThumbnailBatchDownloadResponse, ThumbnailBatchEntry, ThumbnailBatchUploadRequest, ThumbnailPresenceRequest,
    ThumbnailPresenceResponse,
};
use sync_common::{BookId, LibraryId};
use url::{ParseError, Url};

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
    Url(ParseError),
}

impl fmt::Display for AssetResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str("authentication access token was rejected"),
            Self::StorageQuotaExceeded => formatter.write_str("book storage quota exceeded"),
            Self::Retryable(error) | Self::Rejected(error) => error.fmt(formatter),
            Self::Wire(error) => error.fmt(formatter),
            Self::Url(error) => error.fmt(formatter),
        }
    }
}

impl Error for AssetResponseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Retryable(error) | Self::Rejected(error) => Some(error),
            Self::Wire(error) => Some(error),
            Self::Url(error) => Some(error),
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

pub fn blob_endpoint(server_url: &Url, library_id: &LibraryId, content_hash: &BookId) -> Result<Url, ParseError> {
    server_url.join(&format!("api/libraries/{library_id}/blobs/{content_hash}"))
}

pub fn thumbnail_batch_endpoint(server_url: &Url, library_id: &LibraryId) -> Result<Url, ParseError> {
    server_url.join(&format!("api/libraries/{library_id}/thumbnail-batch"))
}
pub fn thumbnail_presence_endpoint(server_url: &Url, library_id: &LibraryId) -> Result<Url, ParseError> {
    server_url.join(&format!("api/libraries/{library_id}/thumbnails/presence"))
}
pub fn thumbnail_endpoint(server_url: &Url, library_id: &LibraryId, content_hash: &BookId) -> Result<Url, ParseError> {
    server_url.join(&format!("api/libraries/{library_id}/thumbnails/{content_hash}"))
}

pub async fn thumbnail_presence(client: &reqwest::Client, credentials: &SyncCredentials, library_id: &LibraryId, content_hashes: Vec<BookId>) -> Result<ThumbnailPresenceResponse, SyncRequestError> {
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
    client: &reqwest::Client, server_url: &Url, access_token: &str, library_id: &LibraryId, identity: &BookId, content_length: u64, body: reqwest::Body,
) -> Result<(), AssetResponseError> {
    let mut trace = PerformanceTrace::new("book_http_upload", "send_body_wait_response");
    log::debug!(target: "sync_performance", "trace_id={} library_id={} bytes={content_length}", trace.id(), library_id);
    let upload_started = web_time::Instant::now();
    let response = client
        .put(blob_endpoint(server_url, library_id, identity).map_err(AssetResponseError::Url)?)
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

pub async fn upload_thumbnail_batch(client: &reqwest::Client, server_url: &Url, access_token: &str, library_id: &LibraryId, thumbnails: Vec<ThumbnailBatchEntry>) -> Result<(), AssetResponseError> {
    let mut trace = PerformanceTrace::new("thumbnail_http_upload", "encode");
    log::debug!(target: "sync_performance", "trace_id={} library_id={} thumbnails={}", trace.id(), library_id, thumbnails.len());
    let request =
        encoded_request(client.put(thumbnail_batch_endpoint(server_url, library_id).map_err(AssetResponseError::Url)?).header("x-bokheim-trace-id", trace.id()).bearer_auth(access_token), &ThumbnailBatchUploadRequest { thumbnails })
            .map_err(AssetResponseError::Wire)?;
    trace.phase("send_body_wait_response");
    let response = request.send().await.map_err(AssetResponseError::Retryable)?;
    log::debug!(target: "sync_performance", "trace_id={} http_status={}", trace.id(), response.status().as_u16());
    let result = validate_streaming_asset_response(response);
    trace.finish(result.is_ok());
    result?;
    Ok(())
}

pub async fn download_thumbnail_batch(client: &reqwest::Client, server_url: &Url, access_token: &str, library_id: &LibraryId, content_hashes: Vec<BookId>) -> Result<ThumbnailBatchDownloadResponse, AssetResponseError> {
    let request =
        encoded_request(client.post(thumbnail_batch_endpoint(server_url, library_id).map_err(AssetResponseError::Url)?).bearer_auth(access_token), &ThumbnailBatchDownloadRequest { content_hashes }).map_err(AssetResponseError::Wire)?;
    let response = request.send().await.map_err(AssetResponseError::Retryable)?;
    let response = validate_streaming_asset_response(response)?;
    let bytes = crate::body::read(response, sync_common::api::assets::MAX_THUMBNAIL_BATCH_DOWNLOAD_BYTES).await.map_err(AssetResponseError::from)?;
    sync_common::transport::decode(&bytes, sync_common::api::assets::MAX_THUMBNAIL_BATCH_DOWNLOAD_BYTES).map_err(AssetResponseError::Wire)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use axum::{body::{Body, Bytes}, response::Response, routing::post, Router};
    use futures_util::StreamExt;
    use std::{convert::Infallible, time::Duration};
    use sync_common::{api::assets::MAX_THUMBNAIL_BATCH_DOWNLOAD_BYTES, transport::WireError};

    #[test]
    fn asset_endpoints_reject_opaque_origins_and_preserve_server_prefixes() {
        let library = LibraryId::from_u128(1);
        let hash = BookId::from_bytes([0xab; 32]);
        let invalid = Url::parse("data:text/plain,invalid-origin").unwrap();
        for endpoint in [blob_endpoint(&invalid, &library, &hash), thumbnail_batch_endpoint(&invalid, &library), thumbnail_presence_endpoint(&invalid, &library), thumbnail_endpoint(&invalid, &library, &hash)] {
            assert_eq!(endpoint.unwrap_err(), ParseError::RelativeUrlWithCannotBeABaseBase);
        }
        let origin = Url::parse("https://example.com/reader/").unwrap();
        for (endpoint, path) in [
            (blob_endpoint(&origin, &library, &hash), format!("blobs/{hash}")),
            (thumbnail_batch_endpoint(&origin, &library), "thumbnail-batch".into()),
            (thumbnail_presence_endpoint(&origin, &library), "thumbnails/presence".into()),
            (thumbnail_endpoint(&origin, &library, &hash), format!("thumbnails/{hash}")),
        ] {
            assert_eq!(endpoint.unwrap().as_str(), format!("https://example.com/reader/api/libraries/{library}/{path}"));
        }
    }

    #[tokio::test]
    async fn asset_requests_return_url_errors_before_sending() {
        let client = reqwest::Client::new();
        let origin = Url::parse("data:text/plain,invalid-origin").unwrap();
        let library = LibraryId::from_u128(1);
        let hash = BookId::from_bytes([0xab; 32]);
        let upload = upload_book_stream(&client, &origin, "test-token", &library, &hash, 1, reqwest::Body::from("x")).await;
        assert!(matches!(upload, Err(AssetResponseError::Url(_))));
        let upload = upload_thumbnail_batch(&client, &origin, "test-token", &library, Vec::new()).await;
        assert!(matches!(upload, Err(AssetResponseError::Url(_))));
        let download = download_thumbnail_batch(&client, &origin, "test-token", &library, Vec::new()).await;
        assert!(matches!(download, Err(AssetResponseError::Url(_))));
    }

    #[tokio::test]
    async fn thumbnail_requests_preserve_methods_headers_and_payloads() {
        use axum::http::HeaderMap;
        use reqwest::header;
        use sync_common::transport as wire;

        fn check_headers(headers: &HeaderMap) {
            assert_eq!(headers.get(header::AUTHORIZATION).unwrap(), "Bearer test-token");
            assert_eq!(headers.get(header::ACCEPT).unwrap(), wire::MEDIA_TYPE);
            assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), wire::MEDIA_TYPE);
        }
        fn thumbnail() -> ThumbnailBatchEntry {
            ThumbnailBatchEntry { content_hash: BookId::from_bytes([0xab; 32]), bytes: vec![0, 128, 255], browse_bytes: vec![1, 2] }
        }
        let app = Router::new().route(
            "/api/libraries/:id/thumbnail-batch",
            post(|headers: HeaderMap, body: Bytes| async move {
                check_headers(&headers);
                let request: ThumbnailBatchDownloadRequest = wire::decode(&body, wire::MAX_DECODED_REQUEST_BYTES).unwrap();
                assert_eq!(request.content_hashes, vec![thumbnail().content_hash]);
                wire::encode(&ThumbnailBatchDownloadResponse { thumbnails: vec![thumbnail()], missing: Vec::new() }).unwrap()
            })
            .put(|headers: HeaderMap, body: Bytes| async move {
                check_headers(&headers);
                assert!(headers.contains_key("x-bokheim-trace-id"));
                let request: ThumbnailBatchUploadRequest = wire::decode(&body, wire::MAX_DECODED_REQUEST_BYTES).unwrap();
                assert_eq!(request.thumbnails, vec![thumbnail()]);
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap()).parse().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let library_id = LibraryId::from_u128(1);
        upload_thumbnail_batch(&client, &origin, "test-token", &library_id, vec![thumbnail()]).await.unwrap();
        let response = download_thumbnail_batch(&client, &origin, "test-token", &library_id, vec![thumbnail().content_hash]).await.unwrap();
        assert_eq!(response, ThumbnailBatchDownloadResponse { thumbnails: vec![thumbnail()], missing: Vec::new() });
        server.abort();
    }

    #[tokio::test]
    async fn thumbnail_download_rejects_oversized_headers_before_waiting_for_the_body() {
        let app = Router::new().route("/api/libraries/:id/thumbnail-batch", post(|| async {
            let body = futures_util::stream::once(async { Ok::<_, Infallible>(Bytes::from_static(b"x")) }).chain(futures_util::stream::pending());
            Response::builder().header("content-length", (MAX_THUMBNAIL_BATCH_DOWNLOAD_BYTES + 1).to_string()).body(Body::from_stream(body)).unwrap()
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}/", listener.local_addr().unwrap()).parse().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let response = tokio::time::timeout(Duration::from_secs(2), download_thumbnail_batch(&reqwest::Client::new(), &origin, "test-token", &LibraryId::from_u128(1), Vec::new())).await.unwrap();
        assert!(matches!(response, Err(AssetResponseError::Wire(WireError::TooLarge { limit })) if limit == MAX_THUMBNAIL_BATCH_DOWNLOAD_BYTES));
        server.abort();
    }
}
