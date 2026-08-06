use crate::extract::{asset_error, AuthUser};
use crate::state::AppState;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use futures_util::{StreamExt, TryStreamExt};
use server_core::AssetKind;
use sync_common::api::assets::{BlobManifestRequest, BlobsExistsRequest, ExistsResponse, ThumbnailsExistsRequest, MAX_BLOB_MANIFEST_ENTRIES};
use sync_common::{ContentHash, LibraryId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

const MAX_EXISTS_HASHES: usize = 4096;
const STREAM_CHUNK_BYTES: usize = 64 * 1024;

async fn head(state: &AppState, user_id: &str, library_id: &LibraryId, kind: AssetKind, raw_hash: &str) -> Result<Response, StatusCode> {
    let hash = ContentHash::parse(raw_hash).map_err(|_| StatusCode::BAD_REQUEST)?;
    let Some(asset) = state.assets.open_from(user_id, library_id, kind, &hash, 0).await.map_err(asset_error)? else {
        return Err(StatusCode::NOT_FOUND);
    };
    let etag = match kind {
        AssetKind::Book => format!("\"{}\"", hash.as_str()),
        AssetKind::Thumbnail => format!("\"cover-{}\"", hash.as_str()),
    };
    let mut response = Response::new(Body::empty());
    response.headers_mut().insert(header::CONTENT_LENGTH, asset.total_len.into());
    response.headers_mut().insert(header::ACCEPT_RANGES, header::HeaderValue::from_static("bytes"));
    response.headers_mut().insert(header::ETAG, header::HeaderValue::from_str(&etag).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
    Ok(response)
}

async fn get(state: &AppState, user_id: &str, library_id: &LibraryId, kind: AssetKind, raw_hash: &str, request_headers: &HeaderMap) -> Result<Response, StatusCode> {
    let hash = ContentHash::parse(raw_hash).map_err(|_| StatusCode::BAD_REQUEST)?;
    let etag = match kind {
        AssetKind::Book => format!("\"{}\"", hash.as_str()),
        AssetKind::Thumbnail => format!("\"cover-{}\"", hash.as_str()),
    };
    let if_range_matches = request_headers.get(header::IF_RANGE).and_then(|value| value.to_str().ok()).is_none_or(|value| value == etag);
    let requested_range = if if_range_matches { request_headers.get(header::RANGE).map(parse_range).transpose()? } else { None };
    let offset = requested_range.map(|range| range.start).unwrap_or(0);
    let Some(asset) = state.assets.open_from(user_id, library_id, kind, &hash, offset).await.map_err(asset_error)? else {
        return Err(StatusCode::NOT_FOUND);
    };
    if requested_range.is_some() && asset.total_len == 0 {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    let governor = state.transfer_governor.clone();
    let throttled_user = user_id.to_string();
    let response_end = requested_range.and_then(|range| range.end).unwrap_or_else(|| asset.total_len.saturating_sub(1));
    if offset >= asset.total_len || response_end < offset {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    let response_len = response_end.min(asset.total_len.saturating_sub(1)) - offset + 1;
    let stream = ReaderStream::with_capacity(asset.reader.take(response_len), STREAM_CHUNK_BYTES).and_then(move |chunk| {
        let governor = governor.clone();
        let user_id = throttled_user.clone();
        async move {
            governor.download(&user_id, chunk.len()).await?;
            Ok(chunk)
        }
    });
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = if requested_range.is_some() { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK };
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_LENGTH, response_len.into());
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(match kind {
            AssetKind::Book => "application/octet-stream",
            AssetKind::Thumbnail => "image/jpeg",
        }),
    );
    headers.insert(header::ACCEPT_RANGES, header::HeaderValue::from_static("bytes"));
    headers.insert(header::ETAG, header::HeaderValue::from_str(&etag).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, header::HeaderValue::from_static("nosniff"));
    if kind == AssetKind::Thumbnail {
        // Cover URLs include the globally owned library id and are immutable
        // within an account, but authenticated responses must never enter a
        // shared intermediary cache.
        headers.insert(header::CACHE_CONTROL, header::HeaderValue::from_static("private, max-age=31536000, immutable, no-transform"));
    }
    if requested_range.is_some() {
        let end = offset + response_len - 1;
        let value = format!("bytes {offset}-{end}/{}", asset.total_len);
        headers.insert(header::CONTENT_RANGE, header::HeaderValue::from_str(&value).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
    }
    *response.headers_mut() = headers;
    Ok(response)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ByteRange {
    start: u64,
    end: Option<u64>,
}

fn parse_range(value: &header::HeaderValue) -> Result<ByteRange, StatusCode> {
    let value = value.to_str().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?;
    let Some(raw) = value.strip_prefix("bytes=") else { return Err(StatusCode::RANGE_NOT_SATISFIABLE) };
    if raw.contains(',') {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    let Some((start, end)) = raw.split_once('-') else { return Err(StatusCode::RANGE_NOT_SATISFIABLE) };
    if start.is_empty() {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    let start = start.parse::<u64>().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?;
    let end = (!end.is_empty()).then(|| end.parse::<u64>().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)).transpose()?;
    if end.is_some_and(|end| end < start) {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    Ok(ByteRange { start, end })
}

async fn put(state: &AppState, user_id: &str, library_id: &LibraryId, kind: AssetKind, raw_hash: &str, headers: &HeaderMap, body: Body) -> Result<StatusCode, StatusCode> {
    let hash = ContentHash::parse(raw_hash).map_err(|_| StatusCode::BAD_REQUEST)?;
    let content_length = headers.get(header::CONTENT_LENGTH).map(|raw_len| raw_len.to_str().ok().and_then(|value| value.parse::<u64>().ok()).ok_or(StatusCode::BAD_REQUEST)).transpose()?;
    if kind == AssetKind::Book && content_length.is_none() {
        return Err(StatusCode::LENGTH_REQUIRED);
    }
    if let Some(len) = content_length {
        if len > kind.max_bytes() {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
    }
    let governor = state.transfer_governor.clone();
    let throttled_user = user_id.to_string();
    let (mut writer, reader) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let mut stream = body.into_data_stream();
        while let Some(result) = stream.next().await {
            let Ok(chunk) = result else {
                break;
            };
            if governor.upload(&throttled_user, chunk.len()).await.is_err() || writer.write_all(&chunk).await.is_err() {
                break;
            }
        }
    });
    state.assets.put(user_id, library_id, kind, &hash, content_length, Box::new(reader)).await.map_err(asset_error)?;
    Ok(StatusCode::OK)
}

pub(super) async fn head_blob(State(state): State<AppState>, user: AuthUser, Path((raw_library_id, hash)): Path<(String, String)>) -> Result<Response, StatusCode> {
    let library_id = LibraryId::parse(&raw_library_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    head(&state, &user.user_id, &library_id, AssetKind::Book, &hash).await
}

pub(super) async fn get_blob(State(state): State<AppState>, user: AuthUser, Path((raw_library_id, hash)): Path<(String, String)>, headers: HeaderMap) -> Result<Response, StatusCode> {
    let library_id = LibraryId::parse(&raw_library_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    get(&state, &user.user_id, &library_id, AssetKind::Book, &hash, &headers).await
}

pub(super) async fn put_blob(State(state): State<AppState>, user: AuthUser, Path((raw_library_id, hash)): Path<(String, String)>, headers: HeaderMap, body: Body) -> Result<StatusCode, StatusCode> {
    let library_id = LibraryId::parse(&raw_library_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    put(&state, &user.user_id, &library_id, AssetKind::Book, &hash, &headers, body).await
}

pub(super) async fn post_blobs_exists(State(state): State<AppState>, user: AuthUser, Path(raw_library_id): Path<String>, headers: HeaderMap, body: Bytes) -> Result<Response, StatusCode> {
    let request: BlobsExistsRequest = crate::api_wire::decode_request(&headers, &body)?;
    let library_id = LibraryId::parse(&raw_library_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    if request.hashes.len() > MAX_EXISTS_HASHES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let present = state.assets.existing(&user.user_id, &library_id, AssetKind::Book, request.hashes).await.map_err(asset_error)?;
    crate::api_wire::response(&headers, ExistsResponse { present })
}

pub(super) async fn post_blob_manifest(State(state): State<AppState>, user: AuthUser, Path(raw_library_id): Path<String>, headers: HeaderMap, body: Bytes) -> Result<Response, StatusCode> {
    let request: BlobManifestRequest = crate::api_wire::decode_request(&headers, &body)?;
    let library_id = LibraryId::parse(&raw_library_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    if request.blobs.len() > MAX_BLOB_MANIFEST_ENTRIES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let response = state.assets.negotiate_books(&user.user_id, &library_id, request.blobs).await.map_err(asset_error)?;
    crate::api_wire::response(&headers, response)
}

pub(super) async fn head_thumbnail(State(state): State<AppState>, user: AuthUser, Path((raw_library_id, hash)): Path<(String, String)>) -> Result<Response, StatusCode> {
    let library_id = LibraryId::parse(&raw_library_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    head(&state, &user.user_id, &library_id, AssetKind::Thumbnail, &hash).await
}

pub(super) async fn get_thumbnail(State(state): State<AppState>, user: AuthUser, Path((raw_library_id, hash)): Path<(String, String)>, headers: HeaderMap) -> Result<Response, StatusCode> {
    let library_id = LibraryId::parse(&raw_library_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    get(&state, &user.user_id, &library_id, AssetKind::Thumbnail, &hash, &headers).await
}

pub(super) async fn put_thumbnail(State(state): State<AppState>, user: AuthUser, Path((raw_library_id, hash)): Path<(String, String)>, headers: HeaderMap, body: Body) -> Result<StatusCode, StatusCode> {
    let library_id = LibraryId::parse(&raw_library_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    put(&state, &user.user_id, &library_id, AssetKind::Thumbnail, &hash, &headers, body).await
}

pub(super) async fn post_thumbnails_exists(State(state): State<AppState>, user: AuthUser, Path(raw_library_id): Path<String>, headers: HeaderMap, body: Bytes) -> Result<Response, StatusCode> {
    let request: ThumbnailsExistsRequest = crate::api_wire::decode_request(&headers, &body)?;
    let library_id = LibraryId::parse(&raw_library_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    if request.content_hashes.len() > MAX_EXISTS_HASHES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let present = state.assets.existing(&user.user_id, &library_id, AssetKind::Thumbnail, request.content_hashes).await.map_err(asset_error)?;
    crate::api_wire::response(&headers, ExistsResponse { present })
}

#[cfg(test)]
mod tests {
    use super::{parse_range, ByteRange};
    use axum::http::{header, StatusCode};

    #[test]
    fn accepts_one_bounded_or_open_ended_byte_range() {
        assert_eq!(parse_range(&header::HeaderValue::from_static("bytes=42-")).unwrap(), ByteRange { start: 42, end: None });
        assert_eq!(parse_range(&header::HeaderValue::from_static("bytes=42-99")).unwrap(), ByteRange { start: 42, end: Some(99) });
        for invalid in ["items=1-", "bytes=-5", "bytes=9-1", "bytes=1-,3-", "bytes=x-"] {
            assert_eq!(parse_range(&header::HeaderValue::from_str(invalid).unwrap()), Err(StatusCode::RANGE_NOT_SATISFIABLE));
        }
    }
}
