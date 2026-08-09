use crate::{AuthError, Session};
use sync_common::{ContentHash, LibraryId, MAX_BOOK_BYTES, MAX_THUMBNAIL_BYTES};

/// Upload a validated, content-addressed book to the sync service.
///
/// This byte-oriented entry point is intended for server and browser adapters;
/// native clients can keep using their streaming transfer worker.
pub async fn upload_book_bytes(client: &reqwest::Client, session: &Session, library_id: &LibraryId, content_hash: &ContentHash, bytes: Vec<u8>) -> Result<(), AuthError> {
    if bytes.len() as u64 > MAX_BOOK_BYTES {
        return Err(AuthError::InvalidData("book exceeds the transfer limit".to_owned()));
    }
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
    if response.content_length().is_some_and(|length| length > MAX_BOOK_BYTES) {
        return Err(AuthError::InvalidData("book exceeds the transfer limit".to_owned()));
    }
    let bytes = response.bytes().await?.to_vec();
    if bytes.len() as u64 > MAX_BOOK_BYTES {
        return Err(AuthError::InvalidData("book exceeds the transfer limit".to_owned()));
    }
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
    session.server_url().endpoint(&format!("api/libraries/{}/blobs/{}", urlencoding::encode(library_id.as_str()), content_hash.as_str()))
}

fn thumbnail_url(session: &Session, library_id: &LibraryId, content_hash: &ContentHash) -> reqwest::Url {
    session.server_url().endpoint(&format!("api/libraries/{}/thumbnails/{}", urlencoding::encode(library_id.as_str()), content_hash.as_str()))
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, AuthError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(AuthError::BadStatus(status, body))
}
