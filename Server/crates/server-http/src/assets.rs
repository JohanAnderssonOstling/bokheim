use crate::extract::{asset_error, AuthUser};
use crate::state::AppState;
use axum::body::Body;
use axum::extract::{Json, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use futures_util::{StreamExt, TryStreamExt};
use server_core::AssetKind;
use sync_common::api::assets::{BlobsExistsRequest, ExistsResponse, ThumbnailsExistsRequest};
use sync_common::{ContentHash, LibraryId};
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

const MAX_EXISTS_HASHES: usize = 4096;

async fn head(state: &AppState, user_id: &str, library_id: &LibraryId, kind: AssetKind, raw_hash: &str) -> StatusCode {
    let Ok(hash) = ContentHash::parse(raw_hash) else {
        return StatusCode::BAD_REQUEST;
    };
    match state.assets.exists(user_id, library_id, kind, &hash).await {
        Ok(true) => StatusCode::OK,
        Ok(false) => StatusCode::NOT_FOUND,
        Err(error) => asset_error(error),
    }
}

async fn get(state: &AppState, user_id: &str, library_id: &LibraryId, kind: AssetKind, raw_hash: &str, request_headers: &HeaderMap) -> Result<Response, StatusCode> {
    let hash = ContentHash::parse(raw_hash).map_err(|_| StatusCode::BAD_REQUEST)?;
    let etag = format!("\"{}\"", hash.as_str());
    let if_range_matches = request_headers.get(header::IF_RANGE).and_then(|value| value.to_str().ok()).is_none_or(|value| value == etag);
    let requested_range = if if_range_matches { request_headers.get(header::RANGE).map(parse_range_start).transpose()? } else { None };
    let offset = requested_range.unwrap_or(0);
    let Some(asset) = state.assets.open_from(user_id, library_id, kind, &hash, offset).await.map_err(asset_error)? else {
        return Err(StatusCode::NOT_FOUND);
    };
    if requested_range.is_some() && asset.total_len == 0 {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    let governor = state.transfer_governor.clone();
    let throttled_user = user_id.to_string();
    let stream = ReaderStream::new(asset.reader).and_then(move |chunk| {
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
    headers.insert(header::CONTENT_LENGTH, (asset.total_len - asset.offset).into());
    headers.insert(header::CONTENT_TYPE, header::HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::ACCEPT_RANGES, header::HeaderValue::from_static("bytes"));
    headers.insert(header::ETAG, header::HeaderValue::from_str(&etag).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
    if requested_range.is_some() {
        let end = asset.total_len.saturating_sub(1);
        let value = format!("bytes {offset}-{end}/{}", asset.total_len);
        headers.insert(header::CONTENT_RANGE, header::HeaderValue::from_str(&value).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
    }
    *response.headers_mut() = headers;
    Ok(response)
}

fn parse_range_start(value: &header::HeaderValue) -> Result<u64, StatusCode> {
    let value = value.to_str().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?;
    let Some(raw) = value.strip_prefix("bytes=") else { return Err(StatusCode::RANGE_NOT_SATISFIABLE) };
    if raw.contains(',') {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    let Some((start, end)) = raw.split_once('-') else { return Err(StatusCode::RANGE_NOT_SATISFIABLE) };
    if start.is_empty() || !end.is_empty() {
        return Err(StatusCode::RANGE_NOT_SATISFIABLE);
    }
    start.parse::<u64>().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)
}

async fn put(state: &AppState, user_id: &str, library_id: &LibraryId, kind: AssetKind, raw_hash: &str, headers: &HeaderMap, body: Body) -> Result<StatusCode, StatusCode> {
    let hash = ContentHash::parse(raw_hash).map_err(|_| StatusCode::BAD_REQUEST)?;
    let content_length = headers
        .get(header::CONTENT_LENGTH)
        .map(|raw_len| raw_len.to_str().ok().and_then(|value| value.parse::<u64>().ok()).ok_or(StatusCode::BAD_REQUEST))
        .transpose()?;
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

pub(super) async fn head_blob(State(state): State<AppState>, user: AuthUser, Path((raw_library_id, hash)): Path<(String, String)>) -> StatusCode {
    let Ok(library_id) = LibraryId::parse(&raw_library_id) else { return StatusCode::BAD_REQUEST };
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

pub(super) async fn post_blobs_exists(State(state): State<AppState>, user: AuthUser, Path(raw_library_id): Path<String>, Json(request): Json<BlobsExistsRequest>) -> Result<Json<ExistsResponse>, StatusCode> {
    let library_id = LibraryId::parse(&raw_library_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    if request.hashes.len() > MAX_EXISTS_HASHES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let present = state.assets.existing(&user.user_id, &library_id, AssetKind::Book, request.hashes).await.map_err(asset_error)?;
    Ok(Json(ExistsResponse { present }))
}

pub(super) async fn head_thumbnail(State(state): State<AppState>, user: AuthUser, Path((raw_library_id, hash)): Path<(String, String)>) -> StatusCode {
    let Ok(library_id) = LibraryId::parse(&raw_library_id) else { return StatusCode::BAD_REQUEST };
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

pub(super) async fn post_thumbnails_exists(State(state): State<AppState>, user: AuthUser, Path(raw_library_id): Path<String>, Json(request): Json<ThumbnailsExistsRequest>) -> Result<Json<ExistsResponse>, StatusCode> {
    let library_id = LibraryId::parse(&raw_library_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    if request.content_hashes.len() > MAX_EXISTS_HASHES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let present = state.assets.existing(&user.user_id, &library_id, AssetKind::Thumbnail, request.content_hashes).await.map_err(asset_error)?;
    Ok(Json(ExistsResponse { present }))
}

#[cfg(test)]
mod tests {
    use super::parse_range_start;
    use axum::http::{header, StatusCode};

    #[test]
    fn accepts_only_one_open_ended_byte_range() {
        assert_eq!(parse_range_start(&header::HeaderValue::from_static("bytes=42-")).unwrap(), 42);
        for invalid in ["items=1-", "bytes=-5", "bytes=1-2", "bytes=1-,3-", "bytes=x-"] {
            assert_eq!(parse_range_start(&header::HeaderValue::from_str(invalid).unwrap()), Err(StatusCode::RANGE_NOT_SATISFIABLE));
        }
    }
}
