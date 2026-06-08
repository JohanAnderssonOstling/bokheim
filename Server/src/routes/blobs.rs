use axum::body::{Body, Bytes};
use axum::extract::{Json, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::{Path as StdPath, PathBuf};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

use crate::routes::extract::AuthUser;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ExistsRequest {
    hashes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ThumbnailExistsRequest {
    content_hashes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ExistsResponse {
    present: Vec<String>,
}

/// Content-addressed blobs are sharded by the first two hex chars of the hash.
pub fn blob_path(base: &PathBuf, hash: &str) -> PathBuf {
    let pfx = hash.chars().take(2).collect::<String>();
    base.join(pfx).join(hash)
}

pub fn thumbnail_path(base: &PathBuf, content_hash: &str) -> PathBuf {
    base.join(format!("{content_hash}.png"))
}

pub async fn head_blob(State(state): State<AppState>, _user: AuthUser, Path(hash): Path<String>) -> StatusCode {
    if blob_path(&state.books_dir, &hash).exists() {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

pub async fn get_blob(State(state): State<AppState>, _user: AuthUser, Path(hash): Path<String>) -> Result<Response, StatusCode> {
    stream_file(&blob_path(&state.books_dir, &hash)).await
}

pub async fn put_blob(State(state): State<AppState>, _user: AuthUser, Path(hash): Path<String>, body: Body) -> Result<StatusCode, StatusCode> {
    let final_path = blob_path(&state.books_dir, &hash);
    stream_in_verified(&final_path, body, Some(&hash)).await
}

pub async fn post_blobs_exists(State(state): State<AppState>, _user: AuthUser, Json(req): Json<ExistsRequest>) -> Json<ExistsResponse> {
    let present = req.hashes.into_iter().filter(|hash| blob_path(&state.books_dir, hash).exists()).collect();
    Json(ExistsResponse { present })
}

pub async fn head_thumbnail_blob(State(state): State<AppState>, _user: AuthUser, Path(content_hash): Path<String>) -> StatusCode {
    if thumbnail_path(&state.thumbnails_dir, &content_hash).exists() {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

pub async fn get_thumbnail_blob(State(state): State<AppState>, _user: AuthUser, Path(content_hash): Path<String>) -> Result<Response, StatusCode> {
    stream_file(&thumbnail_path(&state.thumbnails_dir, &content_hash)).await
}

pub async fn put_thumbnail_blob(State(state): State<AppState>, _user: AuthUser, Path(content_hash): Path<String>, body: Body) -> Result<StatusCode, StatusCode> {
    // Thumbnails aren't content-hashed by their own bytes (the URL hash is the
    // book's hash), so we don't verify — just stream and atomically swap.
    stream_in_verified(&thumbnail_path(&state.thumbnails_dir, &content_hash), body, None).await
}

pub async fn post_thumbnails_exists(State(state): State<AppState>, _user: AuthUser, Json(req): Json<ThumbnailExistsRequest>) -> Json<ExistsResponse> {
    let present = req.content_hashes.into_iter().filter(|hash| thumbnail_path(&state.thumbnails_dir, hash).exists()).collect();
    Json(ExistsResponse { present })
}

/// Streams the file at `path` as the response body. Returns 404 if missing.
async fn stream_file(path: &StdPath) -> Result<Response, StatusCode> {
    let file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(StatusCode::NOT_FOUND),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let meta = file.metadata().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_LENGTH, meta.len().into());
    headers.insert(header::CONTENT_TYPE, header::HeaderValue::from_static("application/octet-stream"));

    let mut resp = Response::new(body);
    *resp.status_mut() = StatusCode::OK;
    *resp.headers_mut() = headers;
    Ok(resp)
}

/// Streams `body` into `final_path` via a temp file. If `expected_hash` is
/// Some, computes blake3 as it writes and rejects with 400 on mismatch. On
/// success the temp file is atomically renamed onto `final_path`.
async fn stream_in_verified(final_path: &StdPath, body: Body, expected_hash: Option<&str>) -> Result<StatusCode, StatusCode> {
    if let Some(parent) = final_path.parent() {
        if tokio::fs::create_dir_all(parent).await.is_err() {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    let tmp_path = {
        let mut p = final_path.as_os_str().to_owned();
        p.push(".tmp");
        PathBuf::from(p)
    };

    let mut file = match File::create(&tmp_path).await {
        Ok(f) => f,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let mut hasher = expected_hash.map(|_| blake3::Hasher::new());
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk: Bytes = match chunk {
            Ok(c) => c,
            Err(_) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(StatusCode::BAD_REQUEST);
            }
        };
        if let Some(h) = hasher.as_mut() {
            h.update(&chunk);
        }
        if file.write_all(&chunk).await.is_err() {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    if file.flush().await.is_err() || file.sync_all().await.is_err() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    drop(file);

    if let (Some(h), Some(expected)) = (hasher, expected_hash) {
        let computed = h.finalize().to_hex().to_string();
        if computed != expected {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    if tokio::fs::rename(&tmp_path, final_path).await.is_err() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(StatusCode::OK)
}
