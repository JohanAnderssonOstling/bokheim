use crate::extract::{asset_error, AuthUser};
use crate::state::AppState;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use futures_util::{StreamExt, TryStreamExt};
use server_asset_store::AssetKind;
use sync_common::api::assets::{BlobManifestRequest, MAX_BLOB_MANIFEST_ENTRIES};
use sync_common::{ContentHash, LibraryId};
use tokio::io::AsyncReadExt;
use tokio_util::io::{ReaderStream, StreamReader};

const STREAM_CHUNK_BYTES: usize = 64 * 1024;

fn response_headers(kind: AssetKind, hash: &ContentHash) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let etag = match kind {
        AssetKind::Book => format!("\"{hash}\""),
        AssetKind::Thumbnail => format!("\"cover-{hash}\""),
    };
    headers.insert(header::ACCEPT_RANGES, header::HeaderValue::from_static("bytes"));
    headers.insert(header::ETAG, header::HeaderValue::from_str(&etag).expect("validated content hashes produce valid ETags"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, header::HeaderValue::from_static("nosniff"));
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(match kind {
            AssetKind::Book => "application/octet-stream",
            AssetKind::Thumbnail => "image/jpeg",
        }),
    );
    if kind == AssetKind::Thumbnail {
        // Cover URLs include the globally owned library id and are immutable
        // within an account, but authenticated responses must never enter a
        // shared intermediary cache.
        headers.insert(header::CACHE_CONTROL, header::HeaderValue::from_static("private, max-age=31536000, immutable, no-transform"));
    }
    headers
}

async fn head(state: &AppState, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash) -> Result<Response, StatusCode> {
    let Some(asset) = state.assets.open_from(user_id, library_id, kind, hash, 0).await.map_err(asset_error)? else {
        return Err(StatusCode::NOT_FOUND);
    };
    let mut response = Response::new(Body::empty());
    *response.headers_mut() = response_headers(kind, hash);
    response.headers_mut().insert(header::CONTENT_LENGTH, asset.total_len.into());
    Ok(response)
}

async fn get(state: &AppState, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash, request_headers: &HeaderMap) -> Result<Response, StatusCode> {
    let headers = response_headers(kind, hash);
    let if_range_matches = request_headers.get(header::IF_RANGE).is_none_or(|value| Some(value) == headers.get(header::ETAG));
    let requested_range = if if_range_matches { request_headers.get(header::RANGE).map(parse_range).transpose()? } else { None };
    let offset = requested_range.map(|range| range.start).unwrap_or(0);
    let Some(asset) = state.assets.open_from(user_id, library_id, kind, hash, offset).await.map_err(asset_error)? else {
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
    *response.headers_mut() = headers;
    response.headers_mut().insert(header::CONTENT_LENGTH, response_len.into());
    if requested_range.is_some() {
        let end = offset + response_len - 1;
        let value = format!("bytes {offset}-{end}/{}", asset.total_len);
        response.headers_mut().insert(header::CONTENT_RANGE, header::HeaderValue::from_str(&value).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
    }
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

async fn put(state: &AppState, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash, headers: &HeaderMap, body: Body) -> Result<StatusCode, StatusCode> {
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
    let stream = body.into_data_stream().map_err(|error| std::io::Error::other(error.to_string())).and_then(move |chunk| {
        let governor = governor.clone();
        let user_id = throttled_user.clone();
        async move {
            governor.upload(&user_id, chunk.len()).await?;
            Ok(chunk)
        }
    });
    let reader = StreamReader::new(stream.boxed());
    state.assets.put(user_id, library_id, kind, hash, content_length, Box::new(reader)).await.map_err(asset_error)?;
    Ok(StatusCode::OK)
}

pub(super) async fn head_blob(State(state): State<AppState>, user: AuthUser, Path((library_id, hash)): Path<(LibraryId, ContentHash)>) -> Result<Response, StatusCode> {
    head(&state, &user.user_id, &library_id, AssetKind::Book, &hash).await
}

pub(super) async fn get_blob(State(state): State<AppState>, user: AuthUser, Path((library_id, hash)): Path<(LibraryId, ContentHash)>, headers: HeaderMap) -> Result<Response, StatusCode> {
    get(&state, &user.user_id, &library_id, AssetKind::Book, &hash, &headers).await
}

pub(super) async fn put_blob(State(state): State<AppState>, user: AuthUser, Path((library_id, hash)): Path<(LibraryId, ContentHash)>, headers: HeaderMap, body: Body) -> Result<StatusCode, StatusCode> {
    put(&state, &user.user_id, &library_id, AssetKind::Book, &hash, &headers, body).await
}

pub(super) async fn post_blob_manifest(State(state): State<AppState>, user: AuthUser, Path(library_id): Path<LibraryId>, headers: HeaderMap, body: Bytes) -> Result<Response, StatusCode> {
    let request: BlobManifestRequest = server_wire_http::decode_request(&headers, &body, wire::MAX_DECODED_REQUEST_BYTES)?;
    if request.blobs.len() > MAX_BLOB_MANIFEST_ENTRIES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let response = state.assets.negotiate_books(&user.user_id, &library_id, request.blobs).await.map_err(asset_error)?;
    server_wire_http::response(&headers, response)
}

pub(super) async fn head_thumbnail(State(state): State<AppState>, user: AuthUser, Path((library_id, hash)): Path<(LibraryId, ContentHash)>) -> Result<Response, StatusCode> {
    head(&state, &user.user_id, &library_id, AssetKind::Thumbnail, &hash).await
}

pub(super) async fn get_thumbnail(State(state): State<AppState>, user: AuthUser, Path((library_id, hash)): Path<(LibraryId, ContentHash)>, headers: HeaderMap) -> Result<Response, StatusCode> {
    get(&state, &user.user_id, &library_id, AssetKind::Thumbnail, &hash, &headers).await
}

pub(super) async fn put_thumbnail(State(state): State<AppState>, user: AuthUser, Path((library_id, hash)): Path<(LibraryId, ContentHash)>, headers: HeaderMap, body: Body) -> Result<StatusCode, StatusCode> {
    put(&state, &user.user_id, &library_id, AssetKind::Thumbnail, &hash, &headers, body).await
}

#[cfg(test)]
mod tests {
    use super::{parse_range, response_headers, ByteRange};
    use axum::http::{header, StatusCode};
    use server_asset_store::AssetKind;
    use sync_common::ContentHash;

    #[test]
    fn head_and_get_share_canonical_asset_headers() {
        let hash = ContentHash::new(&"a".repeat(64));
        let book = response_headers(AssetKind::Book, &hash);
        assert_eq!(book.get(header::ETAG).unwrap().to_str().unwrap(), format!("\"{hash}\""));
        assert_eq!(book.get(header::CONTENT_TYPE).unwrap(), "application/octet-stream");
        assert!(book.get(header::CACHE_CONTROL).is_none());

        let thumbnail = response_headers(AssetKind::Thumbnail, &hash);
        assert_eq!(thumbnail.get(header::ETAG).unwrap().to_str().unwrap(), format!("\"cover-{hash}\""));
        assert_eq!(thumbnail.get(header::CONTENT_TYPE).unwrap(), "image/jpeg");
        assert!(thumbnail.get(header::CACHE_CONTROL).is_some());
    }

    #[test]
    fn accepts_one_bounded_or_open_ended_byte_range() {
        assert_eq!(parse_range(&header::HeaderValue::from_static("bytes=42-")).unwrap(), ByteRange { start: 42, end: None });
        assert_eq!(parse_range(&header::HeaderValue::from_static("bytes=42-99")).unwrap(), ByteRange { start: 42, end: Some(99) });
        for invalid in ["items=1-", "bytes=-5", "bytes=9-1", "bytes=1-,3-", "bytes=x-"] {
            assert_eq!(parse_range(&header::HeaderValue::from_str(invalid).unwrap()), Err(StatusCode::RANGE_NOT_SATISFIABLE));
        }
    }
}
