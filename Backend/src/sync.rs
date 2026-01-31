use crate::db;
use crate::scanner::LibraryEvent;
use crate::transfer::run_transfer_workers;
use crate::transfer::TransferJob;
use crossbeam_channel::Sender;
use include_sqlite_sql::{impl_sql, include_sql};
use rusqlite::Transaction;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use reqwest;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sync_common::{EventPayload, PullEventsResponse, PushEventsRequest, PushEventsResponse};
use tokio::runtime::Builder as TokioRuntimeBuilder;
use uuid::Uuid;

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

impl SyncManager {
    pub fn new(
        db_pool: DbPool,
        client_id: String,
        server_url: String,
        library_id: String,
        forced_user_id: Option<String>,
        library_path: String,
        event_tx: Option<Sender<LibraryEvent>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let user_id = Self::get_or_create_user_id(&db_pool, forced_user_id)?;

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            db_pool,
            client_id,
            server_url,
            http_client,
            user_id,
            library_id,
            library_path,
            pending_downloads: Arc::new(Mutex::new(HashSet::new())),
            event_tx,
        })
    }

    fn get_or_create_user_id(
        db_pool: &DbPool,
        forced_user_id: Option<String>,
    ) -> Result<String, Box<dyn std::error::Error>> {
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

    fn get_or_create_global_user_id(
        forced_user_id: Option<String>,
    ) -> Result<Option<String>, Box<dyn std::error::Error>> {
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
            db::get_sync_metadata(&conn, "last_pull_server_seq")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0)
        };

        // Push local events to server
        self.push_local_events().await?;

        // Pull new events from server since last pull
        self.pull_events_from_server(last_pull_server_seq).await?;

        // Sync book assets (files + thumbnails)
        self.sync_assets().await?;

        Ok(())
    }

    fn emit_event(&self, event: LibraryEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event);
        }
    }

    async fn sync_assets(&self) -> Result<(), Box<dyn std::error::Error>> {
        todo!()
        /*
        let db_pool = self.db.pool();
        let conn = db_pool.get()?;
        let mut book_paths = Vec::new();
        /*conn.get_book_paths_with_downloaded(|row| {
            let book_id: String = row.get(0)?;
            let file_path: String = row.get(1)?;
            let is_downloaded: i64 = row.get(2)?;
            book_paths.push((book_id, file_path, is_downloaded != 0));
            Ok(())
        })?;*/

        let mut jobs = Vec::new();

        for (book_id, file_path, is_downloaded) in book_paths {
            let local_path = self.resolve_book_path(&file_path);
            if Path::new(&local_path).exists() {
                jobs.push(TransferJob::UploadBook {
                    book_id: book_id.clone(),
                    rel_path: file_path.clone(),
                    local_path: local_path.clone(),
                });
                jobs.push(TransferJob::UploadThumbnail {
                    book_id: book_id.clone(),
                    rel_path: file_path.clone(),
                    local_path: local_path.clone(),
                });
                if !is_downloaded {
                    self.db.set_book_downloaded(&file_path, &book_id, true);
                }
            } else if is_downloaded {
                self.db.set_book_downloaded(&file_path, &book_id, false);
            }
        }

        // Pending downloads
        let pending: Vec<String> = {
            let mut set = self.pending_downloads.lock().unwrap();
            set.drain().collect()
        };
        for book_id in pending {
            let mut file_path: Option<String> = None;
            let _ = conn.get_book_file_path(&book_id, |row| {
                file_path = Some(row.get(0)?);
                Ok(())
            });
            let Some(file_path) = file_path else {
                continue;
            };
            let local_path = self.resolve_book_path(&file_path);
            jobs.push(TransferJob::DownloadBook {
                book_id: book_id.clone(),
                rel_path: file_path.clone(),
                local_path: local_path.clone(),
            });
            jobs.push(TransferJob::DownloadThumbnail {
                book_id: book_id.clone(),
            });
        }

        // Ensure thumbnails for known books
        let mut unique_ids = Vec::new();
        conn.get_book_ids(|row| {
            unique_ids.push(row.get::<_, String>(0)?);
            Ok(())
        })?;
        for book_id in unique_ids {
            jobs.push(TransferJob::DownloadThumbnail {
                book_id: book_id.clone(),
            });
        }

        run_transfer_workers(
            self.http_client.clone(),
            self.server_url.clone(),
            self.library_path.clone(),
            db_pool.clone(),
            self.event_tx.clone(),
            jobs,
            4,
        )
        .await?;
        Ok(())

         */
    }

    pub fn request_download(&self, book_id: &str) {
        let mut set = self.pending_downloads.lock().unwrap();
        let should_spawn = set.insert(book_id.to_string());
        drop(set);
        if should_spawn {
            if let Some(job) = self.build_download_job(book_id) {
                self.spawn_manual_download(job);
            }
        }
    }

    fn resolve_book_path(&self, rel_path: &str) -> String {
        let trimmed = rel_path.trim_start_matches('/');
        Path::new(&self.library_path)
            .join(trimmed)
            .to_string_lossy()
            .into_owned()
    }

    fn build_download_job(&self, book_id: &str) -> Option<TransferJob> {
        let conn = self.db_pool.get().ok()?;
        let file_path = db::get_book_file_path(&conn, book_id)?;
        let local_path = self.resolve_book_path(&file_path);
        Some(TransferJob::DownloadBook {
            book_id: book_id.to_string(),
            rel_path: file_path,
            local_path,
        })
    }

    fn spawn_manual_download(&self, job: TransferJob) {
        let http_client = self.http_client.clone();
        let server_url = self.server_url.clone();
        let library_path = self.library_path.clone();
        let db_pool = self.db_pool.clone();
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let runtime = TokioRuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for download");
            runtime.block_on(async move {
                if let Err(err) = run_transfer_workers(
                    http_client,
                    server_url,
                    library_path,
                    db_pool,
                    event_tx,
                    vec![job],
                    1,
                )
                .await
                {
                    log::error!("Manual download failed: {}", err);
                }
            });
        });
    }

    async fn push_local_events(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Get unpushed events for this client
        let events = {
            let conn = self.db_pool.get()?;
            db::get_unpushed_events(&conn)
        };

        if events.is_empty() {
            return Ok(());
        }

        // Track max local_seq for marking as pushed
        let max_local_seq = events.iter().filter_map(|e| e.local_seq).max().unwrap_or(0);

        // Send events to server
        let request = PushEventsRequest {
            user_id: self.user_id.clone(),
            library_id: self.library_id.clone(),
            client_id: self.client_id.clone(),
            events: events.clone(),
        };

        let response = self
            .http_client
            .post(&format!("{}/api/sync/push", self.server_url))
            .json(&request)
            .send()
            .await?
            .error_for_status()?;

        let push_response: PushEventsResponse = response.json().await?;

        // Mark events as pushed with the server_seq
        let conn = self.db_pool.get()?;
        db::mark_events_pushed(&conn, max_local_seq, push_response.last_server_seq);

        // Apply authoritative events from server (server's decision on MPW conflicts)
        for event in &push_response.authoritative_events {
            db::apply_authoritative_event(&conn, event);
        }

        log::info!("Successfully pushed {} events", events.len());
        Ok(())
    }

    async fn pull_events_from_server(
        &self,
        since_server_seq: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let response = self
            .http_client
            .get(&format!(
                "{}/api/sync/pull?user_id={}&library_id={}&since_server_seq={}",
                self.server_url, self.user_id, self.library_id, since_server_seq
            ))
            .send()
            .await?
            .error_for_status()?;

        let pull_response: PullEventsResponse = response.json().await?;
        let events = pull_response.events;

        if events.is_empty() {
            return Ok(());
        }

        let mut conn = self.db_pool.get()?;
        let tx = conn.transaction()?;

        // Apply events from server (server is authoritative)
        for event in events.iter() {
            db::apply_remote_event_in_tx(&tx, event)?;

            // Emit UI events for relevant changes
            match &event.payload {
                EventPayload::DirAdded { dir_id, .. } => {
                    if let Some(path) = Self::resolve_dir_path(&tx, dir_id) {
                        self.emit_event(LibraryEvent::DirectoryAdded { path });
                    }
                }
                EventPayload::BookDirAdded {
                    book_id,
                    dir_id,
                    file_name,
                    ..
                } => {
                    if let Some(dir_path) = Self::resolve_dir_path(&tx, dir_id) {
                        let path = Self::join_dir_and_file(&dir_path, file_name);
                        self.emit_event(LibraryEvent::BookAdded {
                            book_id: book_id.to_string(),
                            path,
                        });
                    }
                }
                EventPayload::BookDirRemoved {
                    book_id,
                    dir_id,
                    file_name,
                    ..
                } => {
                    if let Some(dir_path) = Self::resolve_dir_path(&tx, dir_id) {
                        let path = Self::join_dir_and_file(&dir_path, file_name);
                        self.emit_event(LibraryEvent::BookRemoved {
                            book_id: book_id.to_string(),
                            path,
                        });
                    }
                }
                _ => {}
            }
        }

        // Update last pull cursor to highest server_seq
        if let Some(last_event) = events.last() {
            if let Some(server_seq) = last_event.server_seq {
                db::set_sync_metadata(&tx, "last_pull_server_seq", &server_seq.to_string());
            }
        }

        tx.commit()?;
        log::info!("Successfully pulled {} events", events.len());

        Ok(())
    }

    fn resolve_dir_path(tx: &Transaction, dir_id: &str) -> Option<String> {
        let mut path = None;
       // let _ = tx.find_path_by_dir_id(dir_id, |row| {
          //  path = Some(row.get::<_, String>(0)?);
         //   Ok(())
       // });
        path
    }

    fn join_dir_and_file(dir_path: &str, file_name: &str) -> String {
        if dir_path == "/" {
            format!("/{file_name}")
        } else {
            format!("{dir_path}/{file_name}")
        }
    }
}
