use crate::api::send;
use crate::state::SyncRequestError;
use crate::{AuthError, ServerUrl, Session};
use std::{error::Error, fmt};
use sync_common::api::assets::{validate_blob_manifest_response, BlobManifestEntry, BlobManifestRequest, BlobManifestResponse};
use sync_common::{ContentHash, LibraryId, MAX_THUMBNAIL_BYTES};

/// Failures that can be classified without consuming a streaming asset body.
///
/// Native clients keep ownership of file IO and resumable downloads, while
/// this crate owns the sync service's HTTP status semantics.
#[derive(Debug)]
pub enum AssetResponseError {
    AuthenticationRequired,
    StorageQuotaExceeded,
    Retryable(reqwest::Error),
    Rejected(reqwest::Error),
}

/// The transport-level outcomes of a resumable blob download. File staging,
/// hashing, and publication remain the native backend's responsibility.
#[derive(Debug)]
pub enum BlobDownloadResponse {
    Missing,
    RestartRequired,
    Content(reqwest::Response),
}

impl fmt::Display for AssetResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str("authentication access token was rejected"),
            Self::StorageQuotaExceeded => formatter.write_str("book storage quota exceeded"),
            Self::Retryable(error) | Self::Rejected(error) => error.fmt(formatter),
        }
    }
}

impl Error for AssetResponseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Retryable(error) | Self::Rejected(error) => Some(error),
            Self::AuthenticationRequired | Self::StorageQuotaExceeded => None,
        }
    }
}

/// Applies the sync service's common status rules while preserving the body
/// for a streaming native client.
pub fn validate_streaming_asset_response(response: reqwest::Response) -> Result<reqwest::Response, AssetResponseError> {
    match response.status() {
        reqwest::StatusCode::UNAUTHORIZED => Err(AssetResponseError::AuthenticationRequired),
        reqwest::StatusCode::INSUFFICIENT_STORAGE => Err(AssetResponseError::StorageQuotaExceeded),
        _ => response.error_for_status().map_err(classify_http_error),
    }
}

fn classify_http_error(error: reqwest::Error) -> AssetResponseError {
    if error.status().is_some_and(|status| status.is_client_error() && status != reqwest::StatusCode::REQUEST_TIMEOUT && status != reqwest::StatusCode::TOO_MANY_REQUESTS) {
        AssetResponseError::Rejected(error)
    } else {
        AssetResponseError::Retryable(error)
    }
}

pub fn blob_endpoint(server_url: &ServerUrl, library_id: &LibraryId, content_hash: &ContentHash) -> reqwest::Url {
    server_url.endpoint(&format!("api/libraries/{library_id}/blobs/{}", urlencoding::encode(content_hash.as_str())))
}

pub fn thumbnail_endpoint(server_url: &ServerUrl, library_id: &LibraryId, content_hash: &ContentHash) -> reqwest::Url {
    server_url.endpoint(&format!("api/libraries/{library_id}/thumbnails/{}", urlencoding::encode(content_hash.as_str())))
}

pub fn thumbnail_batch_endpoint(server_url: &ServerUrl, library_id: &LibraryId) -> reqwest::Url {
    server_url.endpoint(&format!("api/libraries/{library_id}/thumbnail-batch"))
}

pub async fn negotiate_blobs(client: &reqwest::Client, session: &Session, library_id: &LibraryId, blobs: Vec<BlobManifestEntry>) -> Result<BlobManifestResponse, SyncRequestError> {
    let request = BlobManifestRequest { blobs };
    let response: BlobManifestResponse = send(client.post(session.server_url().endpoint(&format!("api/libraries/{library_id}/blobs/negotiate"))).bearer_auth(session.token()), &request).await?;
    validate_blob_manifest_response(&request, &response).map_err(|error| AuthError::InvalidData(error.to_string()))?;
    Ok(response)
}

pub async fn upload_blob_stream(client: &reqwest::Client, server_url: &ServerUrl, access_token: &str, library_id: &LibraryId, content_hash: &ContentHash, content_length: u64, body: reqwest::Body) -> Result<(), AssetResponseError> {
    let response = client.put(blob_endpoint(server_url, library_id, content_hash)).bearer_auth(access_token).header(reqwest::header::CONTENT_LENGTH, content_length).body(body).send().await.map_err(AssetResponseError::Retryable)?;
    validate_streaming_asset_response(response)?;
    Ok(())
}

pub async fn download_blob_stream(client: &reqwest::Client, server_url: &ServerUrl, access_token: &str, library_id: &LibraryId, content_hash: &ContentHash, resume_from: Option<u64>) -> Result<BlobDownloadResponse, AssetResponseError> {
    let mut request = client.get(blob_endpoint(server_url, library_id, content_hash)).bearer_auth(access_token);
    if let Some(offset) = resume_from.filter(|offset| *offset > 0) {
        request = request.header(reqwest::header::RANGE, format!("bytes={offset}-")).header(reqwest::header::IF_RANGE, format!("\"{}\"", content_hash.as_str()));
    }
    let response = request.send().await.map_err(AssetResponseError::Retryable)?;
    match response.status() {
        reqwest::StatusCode::NOT_FOUND => Ok(BlobDownloadResponse::Missing),
        reqwest::StatusCode::RANGE_NOT_SATISFIABLE if resume_from.is_some_and(|offset| offset > 0) => Ok(BlobDownloadResponse::RestartRequired),
        _ => validate_streaming_asset_response(response).map(BlobDownloadResponse::Content),
    }
}

pub async fn upload_thumbnail_batch_body(client: &reqwest::Client, server_url: &ServerUrl, access_token: &str, library_id: &LibraryId, body: Vec<u8>) -> Result<(), AssetResponseError> {
    let response = client
        .put(thumbnail_batch_endpoint(server_url, library_id))
        .bearer_auth(access_token)
        .header(reqwest::header::ACCEPT, sync_common::transport::MEDIA_TYPE)
        .header(reqwest::header::CONTENT_TYPE, sync_common::transport::MEDIA_TYPE)
        .body(body)
        .send()
        .await
        .map_err(AssetResponseError::Retryable)?;
    validate_streaming_asset_response(response)?;
    Ok(())
}

pub async fn download_thumbnail_batch_body(client: &reqwest::Client, server_url: &ServerUrl, access_token: &str, library_id: &LibraryId, body: Vec<u8>) -> Result<Vec<u8>, AssetResponseError> {
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
    response.bytes().await.map(|bytes| bytes.to_vec()).map_err(AssetResponseError::Retryable)
}

/// Upload a validated, content-addressed book to the sync service.
///
/// This byte-oriented entry point is intended for server and browser adapters;
/// native clients can keep using their streaming transfer worker.
pub async fn upload_book_bytes(client: &reqwest::Client, session: &Session, library_id: &LibraryId, content_hash: &ContentHash, bytes: Vec<u8>) -> Result<(), AuthError> {
    let actual = blake3::hash(&bytes).to_hex().to_string();
    if actual != content_hash.as_str() {
        return Err(AuthError::InvalidData("book content hash does not match its asset key".to_owned()));
    }
    let response = client.put(asset_url(session, library_id, content_hash)).bearer_auth(session.token()).header(reqwest::header::CONTENT_LENGTH, bytes.len()).body(bytes).send().await?;
    ensure_success(response).await?;
    Ok(())
}

/// Download and verify a complete content-addressed book from the sync service.
pub async fn download_book_bytes(client: &reqwest::Client, session: &Session, library_id: &LibraryId, content_hash: &ContentHash) -> Result<Vec<u8>, AuthError> {
    let response = client.get(asset_url(session, library_id, content_hash)).bearer_auth(session.token()).send().await?;
    let response = ensure_success(response).await?;
    let bytes = response.bytes().await?.to_vec();
    let actual = blake3::hash(&bytes).to_hex().to_string();
    if actual != content_hash.as_str() {
        return Err(AuthError::InvalidData("downloaded book failed content-hash verification".to_owned()));
    }
    Ok(bytes)
}

/// Downloads the thumbnail associated with a book content hash.
///
/// The sync service treats covers as private opaque bytes and enforces only the
/// transfer bound. This client also checks the JPEG signature; applications must
/// validate/decode within their own image limits before display.
pub async fn download_thumbnail_bytes(client: &reqwest::Client, session: &Session, library_id: &LibraryId, content_hash: &ContentHash) -> Result<Vec<u8>, AuthError> {
    let response = client.get(thumbnail_url(session, library_id, content_hash)).bearer_auth(session.token()).send().await?;
    let response = ensure_success(response).await?;
    if response.content_length().is_some_and(|length| length > MAX_THUMBNAIL_BYTES) {
        return Err(AuthError::InvalidData("thumbnail exceeds the transfer limit".to_owned()));
    }
    let bytes = response.bytes().await?.to_vec();
    if bytes.len() as u64 > MAX_THUMBNAIL_BYTES {
        return Err(AuthError::InvalidData("thumbnail exceeds the transfer limit".to_owned()));
    }
    if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Err(AuthError::InvalidData("thumbnail is not a JPEG image".to_owned()));
    }
    Ok(bytes)
}

fn asset_url(session: &Session, library_id: &LibraryId, content_hash: &ContentHash) -> reqwest::Url {
    blob_endpoint(session.server_url(), library_id, content_hash)
}

fn thumbnail_url(session: &Session, library_id: &LibraryId, content_hash: &ContentHash) -> reqwest::Url {
    thumbnail_endpoint(session.server_url(), library_id, content_hash)
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, AuthError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(AuthError::BadStatus(status, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::StatusCode, routing::get, Router};

    #[test]
    fn native_asset_endpoints_share_the_canonical_service_paths() {
        let server_url = ServerUrl::parse("https://example.com").unwrap();
        let library_id = test_support::fixture_library_id("library");
        let content_hash = test_support::fixture_content_hash(7);

        assert_eq!(blob_endpoint(&server_url, &library_id, &content_hash).path(), format!("/api/libraries/{library_id}/blobs/{content_hash}"));
        assert_eq!(thumbnail_endpoint(&server_url, &library_id, &content_hash).path(), format!("/api/libraries/{library_id}/thumbnails/{content_hash}"));
        assert_eq!(thumbnail_batch_endpoint(&server_url, &library_id).path(), format!("/api/libraries/{library_id}/thumbnail-batch"));
    }

    #[tokio::test]
    async fn streaming_statuses_have_closed_authentication_and_quota_semantics() {
        let app = Router::new().route("/unauthorized", get(|| async { StatusCode::UNAUTHORIZED })).route("/quota", get(|| async { StatusCode::INSUFFICIENT_STORAGE })).route("/missing", get(|| async { StatusCode::NOT_FOUND }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();

        let unauthorized = validate_streaming_asset_response(client.get(format!("http://{address}/unauthorized")).send().await.unwrap()).unwrap_err();
        assert!(matches!(unauthorized, AssetResponseError::AuthenticationRequired));
        let quota = validate_streaming_asset_response(client.get(format!("http://{address}/quota")).send().await.unwrap()).unwrap_err();
        assert!(matches!(quota, AssetResponseError::StorageQuotaExceeded));
        let missing = validate_streaming_asset_response(client.get(format!("http://{address}/missing")).send().await.unwrap()).unwrap_err();
        assert!(matches!(missing, AssetResponseError::Rejected(error) if error.status() == Some(reqwest::StatusCode::NOT_FOUND)));
        server.abort();
    }
}
