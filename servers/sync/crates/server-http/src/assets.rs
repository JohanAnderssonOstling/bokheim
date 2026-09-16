use crate::extract::{asset_error, AuthUser};
use crate::state::AppState;
use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use futures_util::{StreamExt, TryStreamExt};
use server_asset_store::AssetKind;
use std::collections::HashSet;
use std::io::Cursor;
use sync_common::api::assets::{
    BlobManifestRequest, ThumbnailBatchDownloadRequest, ThumbnailBatchDownloadResponse, ThumbnailBatchEntry, ThumbnailBatchUploadRequest, MAX_BLOB_MANIFEST_ENTRIES, MAX_THUMBNAIL_BATCH_BYTES, MAX_THUMBNAIL_BATCH_ENTRIES,
};
use sync_common::{ContentHash, LibraryId};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::{ReaderStream, StreamReader};

const STREAM_CHUNK_BYTES: usize = 64 * 1024;
const THUMBNAIL_BATCH_READ_CONCURRENCY: usize = 8;

fn required_content_length(headers: &HeaderMap) -> Result<u64, StatusCode> {
    headers.get(header::CONTENT_LENGTH).ok_or(StatusCode::LENGTH_REQUIRED)?.to_str().ok().and_then(|value| value.parse::<u64>().ok()).ok_or(StatusCode::BAD_REQUEST)
}

fn required_book_checksum(headers: &HeaderMap) -> Result<ContentHash, StatusCode> {
    headers.get("x-bokheim-content-checksum").and_then(|value| value.to_str().ok()).and_then(|value| value.parse().ok()).ok_or(StatusCode::BAD_REQUEST)
}

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
    *response.headers_mut() = response_headers(kind, &asset.checksum);
    if kind == AssetKind::Book {
        response.headers_mut().insert("x-bokheim-content-checksum", header::HeaderValue::from_str(asset.checksum.as_str()).unwrap());
    }
    response.headers_mut().insert(header::CONTENT_LENGTH, asset.total_len.into());
    Ok(response)
}

async fn get(state: &AppState, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash, request_headers: &HeaderMap) -> Result<Response, StatusCode> {
    let Some(asset) = state.assets.open_from(user_id, library_id, kind, hash, 0).await.map_err(asset_error)? else {
        return Err(StatusCode::NOT_FOUND);
    };
    stream_asset(asset, kind, request_headers, state.transfer_governor.clone(), user_id.to_owned()).await
}

pub(crate) async fn stream_asset(mut asset: server_asset_store::StoredAsset, kind: AssetKind, request_headers: &HeaderMap, governor: crate::bandwidth::TransferGovernor, user_id: String) -> Result<Response, StatusCode> {
    let headers = response_headers(kind, &asset.checksum);
    let if_range_matches = request_headers.get(header::IF_RANGE).is_none_or(|value| Some(value) == headers.get(header::ETAG));
    let requested_range = if if_range_matches { request_headers.get(header::RANGE).map(|value| parse_range(value, asset.total_len)).transpose()? } else { None };
    let offset = requested_range.map(|range| range.start).unwrap_or(0);
    if requested_range.is_some() && asset.total_len == 0 {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    let throttled_user = user_id.to_string();
    let response_end = requested_range.and_then(|range| range.end).unwrap_or_else(|| asset.total_len.saturating_sub(1));
    if offset >= asset.total_len || response_end < offset {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    asset.reader.seek(std::io::SeekFrom::Start(offset)).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
    *response.headers_mut() = response_headers(kind, &asset.checksum);
    if kind == AssetKind::Book {
        response.headers_mut().insert("x-bokheim-content-checksum", header::HeaderValue::from_str(asset.checksum.as_str()).unwrap());
    }
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

fn parse_range(value: &header::HeaderValue, total_len: u64) -> Result<ByteRange, StatusCode> {
    let value = value.to_str().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?;
    let Some(raw) = value.strip_prefix("bytes=") else { return Err(StatusCode::RANGE_NOT_SATISFIABLE) };
    if raw.contains(',') {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    let Some((start, end)) = raw.split_once('-') else { return Err(StatusCode::RANGE_NOT_SATISFIABLE) };
    if start.is_empty() {
        let suffix = end.parse::<u64>().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?;
        if suffix == 0 || total_len == 0 {
            return Err(StatusCode::RANGE_NOT_SATISFIABLE);
        }
        return Ok(ByteRange { start: total_len.saturating_sub(suffix), end: None });
    }
    let start = start.parse::<u64>().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?;
    let end = (!end.is_empty()).then(|| end.parse::<u64>().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)).transpose()?;
    if end.is_some_and(|end| end < start) {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    Ok(ByteRange { start, end })
}

async fn put(state: &AppState, user_id: &str, library_id: &LibraryId, kind: AssetKind, hash: &ContentHash, headers: &HeaderMap, body: Body) -> Result<StatusCode, StatusCode> {
    let content_length = required_content_length(headers)?;
    if let Some(limit) = kind.max_bytes() {
        let len = content_length;
        if len > limit {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
    }
    let governor = state.transfer_governor.clone();
    let throttle_ns = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let measured_throttle = throttle_ns.clone();
    let measure = tracing::enabled!(target: "sync_performance", tracing::Level::DEBUG);
    let throttled_user = user_id.to_string();
    let stream = body.into_data_stream().map_err(|error| std::io::Error::other(error.to_string())).and_then(move |chunk| {
        let governor = governor.clone();
        let user_id = throttled_user.clone();
        let measured_throttle = measured_throttle.clone();
        async move {
            let started = measure.then(std::time::Instant::now);
            governor.upload(&user_id, chunk.len()).await?;
            if let Some(started) = started { measured_throttle.fetch_add(started.elapsed().as_nanos().min(u64::MAX as u128) as u64, std::sync::atomic::Ordering::Relaxed); }
            Ok(chunk)
        }
    });
    let reader = StreamReader::new(stream.boxed());
    if kind == AssetKind::Book {
        let checksum = required_book_checksum(headers)?;
        state.assets.put_book_revision(user_id, library_id, hash, &checksum, content_length, Box::new(reader)).await.map_err(asset_error)?;
    } else {
        state.assets.put(user_id, library_id, kind, hash, Some(content_length), Box::new(reader)).await.map_err(asset_error)?;
    }
    tracing::debug!(target: "sync_performance", phase = "upload_throttle", bytes = content_length, elapsed_ms = throttle_ns.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1_000_000.0);
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
    let content_length = required_content_length(&headers)?;
    if content_length > sync_common::MAX_THUMBNAIL_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let _permit = state.thumbnail_uploads.acquire(&user.user_id, content_length).ok_or(StatusCode::TOO_MANY_REQUESTS)?;
    match tokio::time::timeout(server_asset_store::MAX_THUMBNAIL_UPLOAD_DURATION, put(&state, &user.user_id, &library_id, AssetKind::Thumbnail, &hash, &headers, body)).await {
        Ok(result) => result,
        Err(_) => Err(StatusCode::REQUEST_TIMEOUT),
    }
}

async fn put_thumbnail_batch_inner(state: &AppState, user_id: &str, library_id: &LibraryId, headers: HeaderMap, body: Body, content_length: u64) -> Result<StatusCode, StatusCode> {
    let body = to_bytes(body, MAX_THUMBNAIL_BATCH_BYTES + 1024 * 1024).await.map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    if body.len() as u64 != content_length {
        return Err(StatusCode::BAD_REQUEST);
    }
    let request: ThumbnailBatchUploadRequest = server_wire_http::decode_request(&headers, &body, MAX_THUMBNAIL_BATCH_BYTES + 1024 * 1024)?;
    if request.thumbnails.is_empty() || request.thumbnails.len() > MAX_THUMBNAIL_BATCH_ENTRIES {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut seen = HashSet::with_capacity(request.thumbnails.len());
    let mut total = 0_usize;
    for thumbnail in request.thumbnails {
        if !seen.insert(thumbnail.content_hash.clone()) || thumbnail.bytes.len() > sync_common::MAX_THUMBNAIL_BYTES as usize {
            return Err(StatusCode::BAD_REQUEST);
        }
        total = total.checked_add(thumbnail.bytes.len()).ok_or(StatusCode::PAYLOAD_TOO_LARGE)?;
        if total > MAX_THUMBNAIL_BATCH_BYTES {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        let len = thumbnail.bytes.len() as u64;
        state.assets.put(user_id, library_id, AssetKind::Thumbnail, &thumbnail.content_hash, Some(len), Box::new(Cursor::new(thumbnail.bytes))).await.map_err(asset_error)?;
    }
    Ok(StatusCode::OK)
}

pub(super) async fn put_thumbnail_batch(State(state): State<AppState>, user: AuthUser, Path(library_id): Path<LibraryId>, headers: HeaderMap, body: Body) -> Result<StatusCode, StatusCode> {
    let content_length = required_content_length(&headers)?;
    if content_length > (MAX_THUMBNAIL_BATCH_BYTES + 1024 * 1024) as u64 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let _permit = state.thumbnail_uploads.acquire(&user.user_id, content_length).ok_or(StatusCode::TOO_MANY_REQUESTS)?;
    match tokio::time::timeout(server_asset_store::MAX_THUMBNAIL_UPLOAD_DURATION, put_thumbnail_batch_inner(&state, &user.user_id, &library_id, headers, body, content_length)).await {
        Ok(result) => result,
        Err(_) => Err(StatusCode::REQUEST_TIMEOUT),
    }
}

pub(super) async fn post_thumbnail_batch(State(state): State<AppState>, user: AuthUser, Path(library_id): Path<LibraryId>, headers: HeaderMap, body: Bytes) -> Result<Response, StatusCode> {
    let request: ThumbnailBatchDownloadRequest = server_wire_http::decode_request(&headers, &body, wire::MAX_DECODED_REQUEST_BYTES)?;
    if request.content_hashes.is_empty() || request.content_hashes.len() > MAX_THUMBNAIL_BATCH_ENTRIES {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut seen = HashSet::with_capacity(request.content_hashes.len());
    if request.content_hashes.iter().any(|hash| !seen.insert(*hash)) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let assets = &state.assets;
    let user_id = &user.user_id;
    let loaded = futures_util::stream::iter(request.content_hashes.into_iter().map(|hash| async move {
        let Some(mut asset) = assets.open_from(user_id, &library_id, AssetKind::Thumbnail, &hash, 0).await.map_err(asset_error)? else {
            return Ok::<_, StatusCode>((hash, None));
        };
        if asset.total_len > sync_common::MAX_THUMBNAIL_BYTES {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        let mut bytes = Vec::with_capacity(asset.total_len as usize);
        asset.reader.read_to_end(&mut bytes).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok((hash, Some(bytes)))
    }))
    .buffer_unordered(THUMBNAIL_BATCH_READ_CONCURRENCY)
    .try_collect::<Vec<_>>()
    .await?;

    let mut response = ThumbnailBatchDownloadResponse { thumbnails: Vec::new(), missing: Vec::new() };
    let mut total = 0_usize;
    for (hash, bytes) in loaded {
        match bytes {
            Some(bytes) => {
                total = total.checked_add(bytes.len()).ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
                if total > MAX_THUMBNAIL_BATCH_BYTES {
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
                response.thumbnails.push(ThumbnailBatchEntry { content_hash: hash, bytes });
            }
            None => response.missing.push(hash),
        }
    }
    server_wire_http::response(&headers, response)
}

#[cfg(test)]
mod tests {
    use super::{parse_range, response_headers, ByteRange};
    use axum::http::{header, StatusCode};
    use server_asset_store::AssetKind;
    use sync_common::ContentHash;

    #[test]
    fn book_upload_requires_an_explicit_valid_checksum() {
        let mut headers = axum::http::HeaderMap::new();
        assert_eq!(super::required_book_checksum(&headers), Err(StatusCode::BAD_REQUEST));
        headers.insert("x-bokheim-content-checksum", "invalid".parse().unwrap());
        assert_eq!(super::required_book_checksum(&headers), Err(StatusCode::BAD_REQUEST));
        let checksum = ContentHash::new(&"a".repeat(64));
        headers.insert("x-bokheim-content-checksum", checksum.as_str().parse().unwrap());
        assert_eq!(super::required_book_checksum(&headers).unwrap(), checksum);
    }

    #[tokio::test]
    async fn range_responses_are_bound_to_the_actual_revision_checksum() {
        let checksum = ContentHash::new(&"b".repeat(64));
        let payload = b"0123456789abcdef";
        for (range, validator, expected) in [("bytes=4-7", format!("\"{checksum}\""), StatusCode::PARTIAL_CONTENT), ("bytes=4-7", "\"old-revision\"".into(), StatusCode::OK)] {
            let asset = server_asset_store::StoredAsset { reader: Box::new(std::io::Cursor::new(payload.to_vec())), total_len: payload.len() as u64, offset: 0, checksum };
            let mut headers = axum::http::HeaderMap::new(); headers.insert(header::RANGE, range.parse().unwrap()); headers.insert(header::IF_RANGE, validator.parse().unwrap());
            let response = super::stream_asset(asset, AssetKind::Book, &headers, crate::bandwidth::TransferGovernor::new(Default::default()), "test".into()).await.unwrap();
            assert_eq!(response.status(), expected);
            assert_eq!(response.headers().get("x-bokheim-content-checksum").unwrap(), checksum.as_str());
            assert_eq!(response.headers().get(header::ETAG).unwrap().to_str().unwrap(), format!("\"{checksum}\""));
            let bytes = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
            assert_eq!(bytes.as_ref(), if expected == StatusCode::OK { payload.as_slice() } else { &payload[4..8] });
        }
        let asset = server_asset_store::StoredAsset { reader: Box::new(std::io::Cursor::new(payload.to_vec())), total_len: payload.len() as u64, offset: 0, checksum };
        let mut headers = axum::http::HeaderMap::new(); headers.insert(header::RANGE, "bytes=18446744073709551615-".parse().unwrap());
        assert_eq!(super::stream_asset(asset, AssetKind::Book, &headers, crate::bandwidth::TransferGovernor::new(Default::default()), "test".into()).await.unwrap_err(), StatusCode::RANGE_NOT_SATISFIABLE);
    }

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

    #[tokio::test]
    async fn suffix_ranges_stream_the_tail_with_correct_headers() {
        let asset = server_asset_store::StoredAsset {
            reader: Box::new(std::io::Cursor::new(b"0123456789".to_vec())), total_len: 10,
            offset: 0, checksum: ContentHash::new(&"b".repeat(64)),
        };
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(header::RANGE, "bytes=-3".parse().unwrap());
        let response = super::stream_asset(asset, AssetKind::Book, &headers,
            crate::bandwidth::TransferGovernor::new(Default::default()), "test".into()).await.unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 7-9/10");
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "3");
        assert_eq!(axum::body::to_bytes(response.into_body(), 10).await.unwrap().as_ref(), b"789");
    }

    #[test]
    fn accepts_one_bounded_or_open_ended_byte_range() {
        assert_eq!(parse_range(&header::HeaderValue::from_static("bytes=42-"), 100).unwrap(), ByteRange { start: 42, end: None });
        assert_eq!(parse_range(&header::HeaderValue::from_static("bytes=42-99"), 100).unwrap(), ByteRange { start: 42, end: Some(99) });
        assert_eq!(parse_range(&header::HeaderValue::from_static("bytes=-5"), 100).unwrap(), ByteRange { start: 95, end: None });
        assert_eq!(parse_range(&header::HeaderValue::from_static("bytes=-500"), 100).unwrap(), ByteRange { start: 0, end: None });
        assert_eq!(parse_range(&header::HeaderValue::from_static("bytes=-5"), 0), Err(StatusCode::RANGE_NOT_SATISFIABLE));
        for invalid in ["items=1-", "bytes=-0", "bytes=-", "bytes=9-1", "bytes=1-,3-", "bytes=x-"] {
            assert_eq!(parse_range(&header::HeaderValue::from_str(invalid).unwrap(), 100), Err(StatusCode::RANGE_NOT_SATISFIABLE));
        }
    }
}


/// Ordered, concatenated ranges from one pinned publication generation.
pub(super) async fn get_blob_ranges(State(state): State<AppState>, user: AuthUser,
    Path((library_id, hash)): Path<(LibraryId, ContentHash)>, headers: HeaderMap, body: Body) -> Result<Response, StatusCode> {
    let body = to_bytes(body, 64 * 1024).await.map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let request: sync_common::api::assets::PublicationRanges = server_wire_http::decode_request(&headers, &body, 64 * 1024)?;
    let asset = state.assets.open_from(&user.user_id, &library_id, AssetKind::Book, &hash, 0).await.map_err(asset_error)?.ok_or(StatusCode::NOT_FOUND)?;
    stream_ranges(asset, request, state.transfer_governor.clone(), user.user_id).await
}

async fn stream_ranges(asset: server_asset_store::StoredAsset, request: sync_common::api::assets::PublicationRanges,
    governor: crate::bandwidth::TransferGovernor, user: String) -> Result<Response, StatusCode> {
    if request.checksum != asset.checksum { return Err(StatusCode::PRECONDITION_FAILED); }
    let length = request.byte_count(asset.total_len).ok_or(StatusCode::RANGE_NOT_SATISFIABLE)?;
    let headers = response_headers(AssetKind::Book, &asset.checksum);
    let stream = futures_util::stream::try_unfold((asset.reader, request.ranges.into_iter(), 0usize), move |(mut reader, mut ranges, mut remaining)| {
        let governor = governor.clone(); let user = user.clone();
        async move {
            if remaining == 0 {
                let Some((offset, count)) = ranges.next() else { return Ok::<_, std::io::Error>(None) };
                reader.seek(std::io::SeekFrom::Start(offset)).await?;
                remaining = count;
            }
            let mut chunk = vec![0; remaining.min(STREAM_CHUNK_BYTES)];
            reader.read_exact(&mut chunk).await?;
            remaining -= chunk.len();
            governor.download(&user, chunk.len()).await?;
            Ok(Some((Bytes::from(chunk), (reader, ranges, remaining))))
        }
    });
    let mut response = Response::new(Body::from_stream(stream));
    *response.headers_mut() = headers;
    response.headers_mut().insert(header::CONTENT_LENGTH, (length as u64).into());
    Ok(response)
}


#[cfg(test)]
mod bundle_tests {
    use super::*;
    #[tokio::test]
    async fn bundles_are_ordered_bounded_and_pinned_to_the_opened_revision() {
        let checksum = ContentHash::new(&"b".repeat(64));
        let asset = || server_asset_store::StoredAsset { reader: Box::new(Cursor::new(b"0123456789abcdef".to_vec())), total_len: 16, offset: 0, checksum };
        let governor = || crate::bandwidth::TransferGovernor::new(Default::default());
        let request = |ranges| sync_common::api::assets::PublicationRanges { checksum, ranges };
        let response = stream_ranges(asset(), request(vec![(1, 2), (8, 3)]), governor(), "test".into()).await.unwrap();
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "5");
        assert_eq!(axum::body::to_bytes(response.into_body(), 32).await.unwrap().as_ref(), b"1289a");
        for ranges in [vec![], vec![(1, 0)], vec![(1, 5), (4, 2)], vec![(15, 2)], vec![(u64::MAX, 2)]] {
            assert_eq!(stream_ranges(asset(), request(ranges), governor(), "test".into()).await.unwrap_err(), StatusCode::RANGE_NOT_SATISFIABLE);
        }
        let mut wrong = request(vec![(0, 1)]); wrong.checksum = ContentHash::new(&"c".repeat(64));
        assert_eq!(stream_ranges(asset(), wrong, governor(), "test".into()).await.unwrap_err(), StatusCode::PRECONDITION_FAILED);
    }
}
