use crate::scanner::{DownloadState, LibraryEvent};
use crate::sync::DbPool;
use blake3::Hasher;
use futures_util::StreamExt;
use reqwest::Client;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;
use tokio::time::{sleep, Duration, Instant};
use urlencoding::encode;

use crate::db;

#[derive(Debug)]
pub enum TransferJob {
    UploadBlob { hash: String, rel_path: String, local_path: String, book_id: String },
    UploadThumbnail { content_hash: String, thumbnail_path: String, book_id: String },
    DownloadBlob { hash: String, rel_path: String, local_path: String, book_id: String },
    DownloadThumbnail { content_hash: String, book_id: String },
}

pub async fn run_transfer_workers(
    http_client: Client, server_url: String, library_path: String, db_pool: DbPool, event_tx: Option<crossbeam_channel::Sender<LibraryEvent>>, jobs: Vec<TransferJob>, worker_count: usize, bearer_token: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    enqueue_jobs(&db_pool, jobs);

    {
        let conn = db_pool.get()?;
        db::reset_stale_claims(&conn, 300);
    }
    let byte_budget_mb = std::env::var("BIBLOS_TRANSFER_BUDGET_MB").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(64);
    let budget = Arc::new(Semaphore::new(byte_budget_mb.max(1)));
    let mut handles = Vec::new();

    for _ in 0..worker_count.max(1) {
        let client = http_client.clone();
        let server = server_url.clone();
        let lib_path = library_path.clone();
        let pool = db_pool.clone();
        let evt = event_tx.clone();
        let budget = budget.clone();
        let token = bearer_token.clone();
        handles.push(tokio::spawn(async move {
            loop {
                let claimed = {
                    let Ok(conn) = pool.get() else {
                        break;
                    };
                    db::claim_due_transfer_job(&conn)
                };
                let Some(claimed) = claimed else {
                    break;
                };
                let Some(job) = transfer_job_from_row(&claimed) else {
                    if let Ok(conn) = pool.get() {
                        db::fail_transfer_job(&conn, claimed.id, claimed.attempts, "invalid transfer job payload");
                    }
                    continue;
                };
                match handle_job(&client, &server, &lib_path, &pool, evt.as_ref(), &budget, job, token.as_deref()).await {
                    Ok(()) => {
                        if let Ok(conn) = pool.get() {
                            db::complete_transfer_job(&conn, claimed.id);
                        }
                    }
                    Err(err) => {
                        if let Ok(conn) = pool.get() {
                            match claimed.kind.as_str() {
                                "upload_blob" | "download_blob" => db::set_blob_state_error(&conn, "book", &claimed.key1, &err.to_string()),
                                "upload_thumbnail" | "download_thumbnail" => db::set_blob_state_error(&conn, "thumbnail", &claimed.key1, &err.to_string()),
                                _ => {}
                            }
                            db::fail_transfer_job(&conn, claimed.id, claimed.attempts, &err.to_string());
                        }
                        println!("transfer worker error: {}", err);
                    }
                }
            }
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }
    Ok(())
}

fn enqueue_jobs(db_pool: &DbPool, jobs: Vec<TransferJob>) {
    if jobs.is_empty() {
        return;
    }
    let mut jobs = jobs;
    jobs.sort_by_key(transfer_priority);
    let Ok(conn) = db_pool.get() else {
        return;
    };
    let desired: Vec<db::TransferJobKey> = jobs
        .iter()
        .map(|job| match job {
            TransferJob::UploadBlob { hash, rel_path, local_path, book_id } => db::TransferJobKey { kind: "upload_blob".to_string(), key1: hash.clone(), rel_path: rel_path.clone(), local_path: local_path.clone(), book_id: book_id.clone() },
            TransferJob::UploadThumbnail { content_hash, thumbnail_path, book_id } => {
                db::TransferJobKey { kind: "upload_thumbnail".to_string(), key1: content_hash.clone(), rel_path: String::new(), local_path: thumbnail_path.clone(), book_id: book_id.clone() }
            }
            TransferJob::DownloadBlob { hash, rel_path, local_path, book_id } => {
                db::TransferJobKey { kind: "download_blob".to_string(), key1: hash.clone(), rel_path: rel_path.clone(), local_path: local_path.clone(), book_id: book_id.clone() }
            }
            TransferJob::DownloadThumbnail { content_hash, book_id } => db::TransferJobKey { kind: "download_thumbnail".to_string(), key1: content_hash.clone(), rel_path: String::new(), local_path: String::new(), book_id: book_id.clone() },
        })
        .collect();
    db::prune_unclaimed_transfer_jobs(&conn, &desired);

    for job in jobs {
        match job {
            TransferJob::UploadBlob { hash, rel_path, local_path, book_id } => {
                db::upsert_transfer_job(&conn, "upload_blob", &hash, &rel_path, &local_path, &book_id);
            }
            TransferJob::UploadThumbnail { content_hash, thumbnail_path, book_id } => {
                db::upsert_transfer_job(&conn, "upload_thumbnail", &content_hash, "", &thumbnail_path, &book_id);
            }
            TransferJob::DownloadBlob { hash, rel_path, local_path, book_id } => {
                db::upsert_transfer_job(&conn, "download_blob", &hash, &rel_path, &local_path, &book_id);
            }
            TransferJob::DownloadThumbnail { content_hash, book_id } => {
                db::upsert_transfer_job(&conn, "download_thumbnail", &content_hash, "", "", &book_id);
            }
        }
    }
}

fn transfer_job_from_row(row: &db::TransferJobRow) -> Option<TransferJob> {
    match row.kind.as_str() {
        "upload_blob" => Some(TransferJob::UploadBlob { hash: row.key1.clone(), rel_path: row.rel_path.clone(), local_path: row.local_path.clone(), book_id: row.book_id.clone() }),
        "upload_thumbnail" => Some(TransferJob::UploadThumbnail { content_hash: row.key1.clone(), thumbnail_path: row.local_path.clone(), book_id: row.book_id.clone() }),
        "download_blob" => Some(TransferJob::DownloadBlob { hash: row.key1.clone(), rel_path: row.rel_path.clone(), local_path: row.local_path.clone(), book_id: row.book_id.clone() }),
        "download_thumbnail" => Some(TransferJob::DownloadThumbnail { content_hash: row.key1.clone(), book_id: row.book_id.clone() }),
        _ => None,
    }
}

fn transfer_priority(job: &TransferJob) -> (u8, u64) {
    match job {
        TransferJob::DownloadThumbnail { .. } => (0, 0),
        TransferJob::UploadThumbnail { thumbnail_path, .. } => (0, file_size_or(thumbnail_path, 0)),
        TransferJob::UploadBlob { local_path, .. } => (1, file_size_or(local_path, u64::MAX / 2)),
        TransferJob::DownloadBlob { .. } => (1, u64::MAX / 2),
    }
}

fn file_size_or(path: &str, default: u64) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(default)
}

async fn handle_job(client: &Client, server_url: &str, library_path: &str, db_pool: &DbPool, event_tx: Option<&crossbeam_channel::Sender<LibraryEvent>>, budget: &Arc<Semaphore>, job: TransferJob, token: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let token = token.ok_or("transfer job started without auth token")?;
    match job {
        TransferJob::UploadBlob { hash, rel_path, local_path, book_id } => {
            let permits = mb_permits_for_path(&local_path, budget.available_permits()).await;
            let _permit = budget.clone().acquire_many_owned(permits).await?;
            upload_blob(client, server_url, db_pool, &hash, &book_id, &local_path, Some(rel_path.as_str()), token).await?;
        }
        TransferJob::UploadThumbnail { content_hash, thumbnail_path, book_id } => {
            let permits = mb_permits_for_path(&thumbnail_path, budget.available_permits()).await;
            let _permit = budget.clone().acquire_many_owned(permits).await?;
            upload_thumbnail(client, server_url, db_pool, &content_hash, &book_id, &thumbnail_path, token).await?;
        }
        TransferJob::DownloadBlob { hash, rel_path, local_path, book_id } => {
            let _permit = budget.clone().acquire_owned().await?;
            download_blob(client, server_url, db_pool, &hash, &book_id, &rel_path, &local_path, event_tx, token).await?;
        }
        TransferJob::DownloadThumbnail { content_hash, book_id } => {
            let _permit = budget.clone().acquire_owned().await?;
            download_thumbnail(client, server_url, db_pool, event_tx, library_path, &content_hash, &book_id, token).await?;
        }
    }
    Ok(())
}

async fn mb_permits_for_path(path: &str, max_permits: usize) -> u32 {
    let cap = max_permits.max(1);
    let size = tokio::fs::metadata(path).await.map(|m| m.len()).unwrap_or(1024 * 1024);
    let mb = ((size + (1024 * 1024 - 1)) / (1024 * 1024)) as usize;
    mb.clamp(1, cap) as u32
}

async fn upload_blob(client: &Client, server_url: &str, db_pool: &DbPool, hash: &str, book_id: &str, local_path: &str, rel_path: Option<&str>, token: &str) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = tokio::fs::read(local_path).await?;
    client.put(&format!("{}/api/blobs/{}", server_url, encode(hash))).bearer_auth(token).body(bytes).send().await?.error_for_status()?;
    let conn = db_pool.get()?;
    db::upsert_blob_local_ref(&conn, "book", hash, local_path, true);
    db::set_blob_state_checked(&conn, "book", hash, 2);
    if let Some(rel) = rel_path {
        db::set_book_downloaded(&conn, rel, book_id, true);
    }
    Ok(())
}

async fn upload_thumbnail(client: &Client, server_url: &str, db_pool: &DbPool, content_hash: &str, _book_id: &str, thumbnail_path: &str, token: &str) -> Result<(), Box<dyn std::error::Error>> {
    if !Path::new(thumbnail_path).exists() {
        return Ok(());
    }
    let bytes = tokio::fs::read(thumbnail_path).await?;
    client.put(&format!("{}/api/thumbnails/{}", server_url, encode(content_hash))).bearer_auth(token).body(bytes).send().await?.error_for_status()?;
    if let Ok(conn) = db_pool.get() {
        db::upsert_blob_local_ref(&conn, "thumbnail", content_hash, thumbnail_path, true);
        db::set_blob_state_checked(&conn, "thumbnail", content_hash, 2);
    }
    Ok(())
}

async fn download_blob(client: &Client, server_url: &str, db_pool: &DbPool, hash: &str, book_id: &str, rel_path: &str, local_path: &str, event_tx: Option<&crossbeam_channel::Sender<LibraryEvent>>, token: &str) -> Result<(), Box<dyn std::error::Error>> {
    send_download_status(event_tx, book_id, DownloadState::Downloading(0.0));
    let response = client.get(&format!("{}/api/blobs/{}", server_url, encode(hash))).bearer_auth(token).send().await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        send_download_status(event_tx, book_id, DownloadState::NotDownloaded);
        return Ok(());
    }
    let response = response.error_for_status()?;

    let total_size = response.content_length().unwrap_or(0);
    let mut stream = response.bytes_stream();
    if let Some(parent) = Path::new(local_path).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp_path = format!("{local_path}.tmp");
    let mut file = tokio::fs::File::create(&tmp_path).await?;
    let mut downloaded: u64 = 0;
    let mut last_progress = 0.0;
    let mut hasher = Hasher::new();
    let bandwidth_limit = std::env::var("BIBLOS_DOWNLOAD_BANDWIDTH").ok().and_then(|value| value.parse::<f64>().ok()).filter(|&limit| limit > 0.0);
    let mut next_allowed = Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(e.into());
            }
        };
        if let Some(limit) = bandwidth_limit {
            let now = Instant::now();
            if now < next_allowed {
                sleep(next_allowed - now).await;
            }
            let chunk_secs = chunk.len() as f64 / limit;
            next_allowed = Instant::now() + Duration::from_secs_f64(chunk_secs);
        }
        hasher.update(&chunk);
        if let Err(e) = file.write_all(&chunk).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(e.into());
        }
        downloaded += chunk.len() as u64;
        let progress = if total_size == 0 { 0.0 } else { (downloaded as f32 / total_size as f32).clamp(0.0, 1.0) };
        if (progress - last_progress).abs() >= 0.02 || progress >= 0.99 {
            send_download_status(event_tx, book_id, DownloadState::Downloading(progress));
            last_progress = progress;
        }
    }
    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    let got_hash = hasher.finalize().to_hex().to_string();
    if got_hash != hash {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        send_download_status(event_tx, book_id, DownloadState::NotDownloaded);
        return Err(format!("download hash mismatch expected={} got={}", hash, got_hash).into());
    }

    if let Err(e) = tokio::fs::rename(&tmp_path, local_path).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(e.into());
    }

    let conn = db_pool.get()?;
    db::upsert_blob_local_ref(&conn, "book", hash, local_path, true);
    db::derive_blob_state_from_local(&conn, "book", hash);
    db::set_book_downloaded(&conn, rel_path, book_id, true);
    send_download_status(event_tx, book_id, DownloadState::Downloaded);
    Ok(())
}

fn send_download_status(event_tx: Option<&crossbeam_channel::Sender<LibraryEvent>>, book_id: &str, state: DownloadState) {
    if let Some(tx) = event_tx {
        let _ = tx.send(LibraryEvent::DownloadStatusChanged { book_id: book_id.to_string(), state });
    }
}

async fn download_thumbnail(client: &Client, server_url: &str, db_pool: &DbPool, event_tx: Option<&crossbeam_channel::Sender<LibraryEvent>>, library_path: &str, content_hash: &str, book_id: &str, token: &str) -> Result<(), Box<dyn std::error::Error>> {
    let thumb_path = thumbnail_path_from_content_hash(library_path, content_hash);
    if Path::new(&thumb_path).exists() {
        if let Some(tx) = event_tx {
            let _ = tx.send(LibraryEvent::ThumbnailGenerated { book_id: book_id.to_string() });
        }
        return Ok(());
    }

    let response = client.get(&format!("{}/api/thumbnails/{}", server_url, encode(content_hash))).bearer_auth(token).send().await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(());
    }
    let response = response.error_for_status()?;
    let bytes = response.bytes().await?;
    if let Some(parent) = Path::new(&thumb_path).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp_path = format!("{thumb_path}.tmp");
    tokio::fs::write(&tmp_path, &bytes).await?;
    if let Err(e) = tokio::fs::rename(&tmp_path, &thumb_path).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(e.into());
    }
    if let Ok(conn) = db_pool.get() {
        db::upsert_blob_local_ref(&conn, "thumbnail", content_hash, &thumb_path, true);
        db::derive_blob_state_from_local(&conn, "thumbnail", content_hash);
    }
    if let Some(tx) = event_tx {
        let _ = tx.send(LibraryEvent::ThumbnailGenerated { book_id: book_id.to_string() });
    }
    Ok(())
}

fn thumbnail_path_from_content_hash(library_path: &str, content_hash: &str) -> String {
    Path::new(library_path).join(".biblos").join("thumbnails").join(format!("{content_hash}.png")).to_string_lossy().into_owned()
}
