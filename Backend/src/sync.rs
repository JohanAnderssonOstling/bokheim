use crate::db;
use crate::scanner::LibraryEvent;
use crate::transfer::run_transfer_workers;
use crate::transfer::TransferJob;
use crossbeam_channel::Sender;
use include_sqlite_sql::{impl_sql, include_sql};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use reqwest;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
// TODO: sync protocol refactoring in progress — old Event types replaced by Change
// use sync_common::{EventPayload, PullEventsResponse, PushEventsRequest, PushEventsResponse};
use sync_common::{PullChangesResponse, PushChangesRequest, PushChangesResponse};
use uuid::Uuid;
use serde::{Deserialize, Serialize};

include_sql!("src/database/library.sql");

pub type DbPool = Pool<SqliteConnectionManager>;

#[derive(Debug)]
pub struct SyncManager {
    db_pool: DbPool,
    client_id: String,
    server_url: String,
    http_client: reqwest::Client,
    user_id: String,
    library_id: String,
    library_path: String,
    pending_downloads: Arc<Mutex<HashSet<String>>>,
    event_tx: Option<Sender<LibraryEvent>>,
}

#[derive(Debug, Serialize)]
struct BlobsExistsRequest {
    hashes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ThumbnailsExistsRequest {
    content_hashes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExistsResponse {
    present: Vec<String>,
}

impl SyncManager {
    pub fn new(db_pool: DbPool, client_id: String, server_url: String, library_id: String, forced_user_id: Option<String>, library_path: String, event_tx: Option<Sender<LibraryEvent>>) -> Result<Self, Box<dyn std::error::Error>> {
        let user_id = Self::get_or_create_user_id(&db_pool, forced_user_id)?;

        let http_client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;

        Ok(Self { db_pool, client_id, server_url, http_client, user_id, library_id, library_path, pending_downloads: Arc::new(Mutex::new(HashSet::new())), event_tx })
    }

    fn get_or_create_user_id(db_pool: &DbPool, forced_user_id: Option<String>) -> Result<String, Box<dyn std::error::Error>> {
        let file_user_id = Self::get_or_create_global_user_id(forced_user_id.clone())?;
        if let Some(id) = file_user_id {
            let conn = db_pool.get()?;
            db::set_sync_metadata(&conn, "user_id", &id);
            return Ok(id);
        }

        let conn = db_pool.get()?;
        if let Some(id) = db::get_sync_metadata(&conn, "user_id") {
            return Ok(id);
        }

        let new_id = forced_user_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        db::set_sync_metadata(&conn, "user_id", &new_id);
        Ok(new_id)
    }

    fn get_or_create_global_user_id(forced_user_id: Option<String>) -> Result<Option<String>, Box<dyn std::error::Error>> {
        let Ok(home) = std::env::var("HOME") else {
            return Ok(None);
        };
        let mut path = PathBuf::from(home);
        path.push(".biblos");
        fs::create_dir_all(&path)?;
        path.push("user_id");

        if let Some(forced) = forced_user_id {
            fs::write(&path, &forced)?;
            return Ok(Some(forced));
        }

        if let Ok(existing) = fs::read_to_string(&path) {
            let trimmed = existing.trim().to_string();
            if !trimmed.is_empty() {
                return Ok(Some(trimmed));
            }
        }

        let new_id = Uuid::new_v4().to_string();
        fs::write(&path, &new_id)?;
        Ok(Some(new_id))
    }

    pub async fn start_sync_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            interval.tick().await;

            if let Err(e) = self.sync_with_server().await {
                log::error!("Sync failed: {}", e);
            }
        }
    }

    pub async fn sync_once(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.sync_with_server().await
    }

    async fn sync_with_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Get last pull server_seq from metadata
        let last_pull_server_seq = {
            let conn = self.db_pool.get()?;
            db::get_sync_metadata(&conn, "last_pull_server_seq").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0)
        };

        // Push local events to server
        self.push_local_events().await?;

        // Pull new events from server since last pull
        self.pull_events_from_server(last_pull_server_seq).await?;

        // Sync book assets (files + thumbnails)
        self.sync_assets().await?;

        Ok(())
    }

    async fn sync_assets(&self) -> Result<(), Box<dyn std::error::Error>> {
        let conn = self.db_pool.get()?;
        let assets = db::get_book_assets(&conn);
        let mut jobs = Vec::new();
        let mut known_hashes = HashSet::new();
        for asset in assets {
            let local_path = self.resolve_book_path(&asset.file_path);
            let thumb_path = self.thumbnail_path_for_content_hash(&asset.local_hash);
            db::upsert_blob_local_ref(&conn, "book", &asset.local_hash, &local_path, Path::new(&local_path).exists());
            db::derive_blob_state_from_local(&conn, "book", &asset.local_hash);
            db::upsert_blob_local_ref(&conn, "thumbnail", &asset.local_hash, &thumb_path, Path::new(&thumb_path).exists());
            db::derive_blob_state_from_local(&conn, "thumbnail", &asset.local_hash);
            known_hashes.insert(asset.local_hash.clone());
        }
        let upload_blob_hashes = db::get_blob_hashes_by_state(&conn, "book", 1);
        let upload_thumb_hashes = db::get_blob_hashes_by_state(&conn, "thumbnail", 1);
        let mut pending_hashes = db::get_pending_download_hashes(&conn);
        drop(conn);

        let existing_blobs = self.check_existing_blobs(upload_blob_hashes.clone()).await?;
        for hash in upload_blob_hashes {
            if existing_blobs.contains(&hash) {
                let conn = self.db_pool.get()?;
                db::set_blob_state_checked(&conn, "book", &hash, 2);
                continue;
            }
            let conn = self.db_pool.get()?;
            let Some(local_path) = db::get_blob_local_path(&conn, "book", &hash) else {
                continue;
            };
            jobs.push(TransferJob::UploadBlob {
                hash,
                rel_path: String::new(),
                local_path,
                book_id: String::new(),
            });
        }
        let existing_thumbs = self.check_existing_thumbnails(upload_thumb_hashes.clone()).await?;
        for content_hash in upload_thumb_hashes {
            if existing_thumbs.contains(&content_hash) {
                let conn = self.db_pool.get()?;
                db::set_blob_state_checked(&conn, "thumbnail", &content_hash, 2);
                continue;
            }
            let conn = self.db_pool.get()?;
            let Some(thumbnail_path) = db::get_blob_local_path(&conn, "thumbnail", &content_hash) else {
                continue;
            };
            jobs.push(TransferJob::UploadThumbnail {
                content_hash,
                thumbnail_path,
                book_id: String::new(),
            });
        }

        let runtime_pending: Vec<String> = {
            let mut set = self.pending_downloads.lock().unwrap();
            set.drain().collect()
        };
        for hash in runtime_pending {
            if !pending_hashes.iter().any(|x| x == &hash) {
                pending_hashes.push(hash);
            }
        }

        // State-driven requested downloads: local-missing + server-present.
        let existing_pending = self.check_existing_blobs(pending_hashes.clone()).await?;
        let mut still_pending = Vec::new();
        for hash in pending_hashes {
            let conn = self.db_pool.get()?;
            if db::is_blob_local(&conn, "book", &hash) {
                continue;
            }
            if existing_pending.contains(&hash) {
                db::set_blob_state_checked(&conn, "book", &hash, 3);
                let target_path = if let Some(path) = db::get_blob_any_path(&conn, "book", &hash) {
                    path
                } else {
                    // Seed blob_local_ref from canonical metadata paths in book_dir.
                    let book_paths = db::get_book_paths_by_local_hash(&conn, &hash);
                    if let Some(chosen) = book_paths.first() {
                        let local = self.resolve_book_path(chosen);
                        db::upsert_blob_local_ref(&conn, "book", &hash, &local, Path::new(&local).exists());
                        local
                    } else {
                        still_pending.push(hash);
                        continue;
                    }
                };
                jobs.push(TransferJob::DownloadBlob {
                    hash: hash.clone(),
                    rel_path: String::new(),
                    local_path: target_path,
                    book_id: hash.clone(),
                });
            }
            still_pending.push(hash);
        }

        // Eager thumbnails from known hashes: local-missing + server-present.
        let known_hash_list: Vec<String> = known_hashes.into_iter().collect();
        let existing_thumb_known = self.check_existing_thumbnails(known_hash_list.clone()).await?;
        for hash in known_hash_list {
            let conn = self.db_pool.get()?;
            if db::is_blob_local(&conn, "thumbnail", &hash) {
                continue;
            }
            if existing_thumb_known.contains(&hash) {
                db::set_blob_state_checked(&conn, "thumbnail", &hash, 3);
                jobs.push(TransferJob::DownloadThumbnail {
                    content_hash: hash.clone(),
                    book_id: hash,
                });
            }
        }

        {
            let conn = self.db_pool.get()?;
            db::set_pending_download_hashes(&conn, &still_pending);
        }

        run_transfer_workers(self.http_client.clone(), self.server_url.clone(), self.library_path.clone(), self.db_pool.clone(), self.event_tx.clone(), jobs, 4).await?;
        Ok(())
    }

    pub fn request_download(&self, book_id: &str) {
        if let Ok(conn) = self.db_pool.get() {
            db::add_pending_download_hash(&conn, book_id);
        } else {
            let mut set = self.pending_downloads.lock().unwrap();
            let _ = set.insert(book_id.to_string());
        }
    }

    fn resolve_book_path(&self, rel_path: &str) -> String {
        let trimmed = rel_path.trim_start_matches('/');
        Path::new(&self.library_path).join(trimmed).to_string_lossy().into_owned()
    }

    fn thumbnail_path_for_content_hash(&self, content_hash: &str) -> String {
        Path::new(&self.library_path)
            .join(".biblos")
            .join("thumbnails")
            .join(format!("{content_hash}.png"))
            .to_string_lossy()
            .into_owned()
    }

    async fn check_existing_blobs(&self, hashes: Vec<String>) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
        if hashes.is_empty() {
            return Ok(HashSet::new());
        }
        let resp = self
            .http_client
            .post(&format!("{}/api/blobs/exists", self.server_url))
            .json(&BlobsExistsRequest { hashes })
            .send()
            .await?
            .error_for_status()?;
        let body: ExistsResponse = resp.json().await?;
        Ok(body.present.into_iter().collect())
    }

    async fn check_existing_thumbnails(&self, content_hashes: Vec<String>) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
        if content_hashes.is_empty() {
            return Ok(HashSet::new());
        }
        let resp = self
            .http_client
            .post(&format!("{}/api/thumbnails/exists", self.server_url))
            .json(&ThumbnailsExistsRequest { content_hashes })
            .send()
            .await?
            .error_for_status()?;
        let body: ExistsResponse = resp.json().await?;
        Ok(body.present.into_iter().collect())
    }

    async fn push_local_events(&self) -> Result<(), Box<dyn std::error::Error>> {
        let changes = {
            let conn = self.db_pool.get()?;
            db::get_unpushed_events(&conn)
        };

        if changes.is_empty() {
            return Ok(());
        }

        let max_local_seq = changes.iter().filter_map(|e| e.local_seq).max().unwrap_or(0);
        let request = PushChangesRequest {
            user_id: self.user_id.clone(),
            library_id: self.library_id.clone(),
            client_id: self.client_id.clone(),
            changes,
        };

        let response = self
            .http_client
            .post(&format!("{}/api/sync/push", self.server_url))
            .json(&request)
            .send()
            .await?
            .error_for_status()?;
        let push_response: PushChangesResponse = response.json().await?;

        let conn = self.db_pool.get()?;
        db::mark_events_pushed(&conn, max_local_seq, push_response.last_server_seq);
        db::set_sync_metadata(&conn, "first_sync_done", "1");
        Ok(())
    }

    async fn pull_events_from_server(&self, since_server_seq: u64) -> Result<(), Box<dyn std::error::Error>> {
        let response = self
            .http_client
            .get(&format!(
                "{}/api/sync/pull?user_id={}&library_id={}&since_server_seq={}",
                self.server_url, self.user_id, self.library_id, since_server_seq
            ))
            .send()
            .await?
            .error_for_status()?;
        let pull_response: PullChangesResponse = response.json().await?;
        {
            let conn = self.db_pool.get()?;
            db::set_sync_metadata(&conn, "first_sync_done", "1");
        }
        let changes = pull_response.changes;

        if changes.is_empty() {
            return Ok(());
        }

        let mut conn = self.db_pool.get()?;
        let tx = conn.transaction()?;
        for change in &changes {
            db::apply_remote_change_in_tx(&tx, change)?;
        }
        db::drain_pending_remote_changes_in_tx(&tx)?;

        if let Some(last) = changes.last() {
            if let Some(server_seq) = last.server_seq {
                db::set_sync_metadata(&tx, "last_pull_server_seq", &server_seq.to_string());
            }
        }

        tx.commit()?;
        Ok(())
    }

}
