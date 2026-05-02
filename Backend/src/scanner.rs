use crate::home::get_app_data_thumbnails_dir;
use crate::db::{derive_blob_state_from_local_tx, get_book_dir_by_hash, insert_book, insert_local_book_dir, update_book_metadata, upsert_blob_local_ref_tx, upsert_dir, BookDir};
use crate::epub::get_epub_id_title_author;
use crate::library::generate_thumbnail_to_app_data;
use crate::library::generate_book_id_blake3;
use notify::{recommended_watcher, Event, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::HashSet;
use std::hash::Hasher;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel as std_channel;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use std::time::{Duration, UNIX_EPOCH};
use std::{fs, thread};
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Clone, Serialize)]
pub enum DownloadState {
    NotDownloaded,
    Downloading(f32),
    Downloaded,
}

#[derive(Debug, Clone, Serialize)]
pub enum LibraryEvent {
    BookAdded { book_id: String, path: String },
    BookRemoved { book_id: String, path: String },
    DirectoryAdded { path: String },
    DirectoryRemoved { path: String },
    ThumbnailGenerated { book_id: String },
    DownloadStatusChanged { book_id: String, state: DownloadState },
    BookOpened { book_id: String },
    ScanComplete,
}

pub struct LibraryScanner {
    state: Arc<ScannerState>,
    watcher_thread: Option<thread::JoinHandle<()>>,
    worker_thread: Option<thread::JoinHandle<()>>,
    thumbnail_thread: Option<thread::JoinHandle<()>>,
    hash_threads: Vec<thread::JoinHandle<()>>,
}

struct ScannerState {
    library_path: String,
    db: Pool<SqliteConnectionManager>,
    event_tx: Sender<LibraryEvent>,
    park_thread: OnceLock<thread::Thread>,
    scan_requested: AtomicBool,
    should_finish: AtomicBool,
    thumbnail_queue: crossbeam_channel::Sender<ThumbnailTask>,
    hash_queue: crossbeam_channel::Sender<HashTask>,
    hash_result_queue: crossbeam_channel::Sender<HashReady>,
    hash_result_rx: crossbeam_channel::Receiver<HashReady>,
    pending_hash_jobs: std::sync::Mutex<HashSet<u64>>,
    thumbnail_inflight: AtomicUsize,
}

struct ThumbnailTask {
    book_id: String,
    epub_path: String,
}

struct HashTask {
    parent_uuid: String,
    file_name: String,
    epub_path: String,
    file_hash: u64,
}

struct HashReady {
    parent_uuid: String,
    file_name: String,
    epub_path: String,
    file_hash: u64,
    book_hash: u64,
    title: Option<String>,
    author: Option<String>,
}

struct ThumbnailStateUpdate {
    hash: String,
    path: String,
    exists: bool,
}

impl LibraryScanner {
    pub fn new(library_path: String, db: Pool<SqliteConnectionManager>, event_tx: Sender<LibraryEvent>) -> Self {
        let (thumbnail_tx, thumbnail_rx) = crossbeam_channel::unbounded::<ThumbnailTask>();
        let (hash_tx, hash_rx) = crossbeam_channel::unbounded::<HashTask>();
        let (hash_result_tx, hash_result_rx) = crossbeam_channel::unbounded::<HashReady>();

        let state = Arc::new(ScannerState {
            library_path,
            db,
            event_tx: event_tx.clone(),
            park_thread: OnceLock::new(),
            scan_requested: AtomicBool::new(true),
            should_finish: AtomicBool::new(false),
            thumbnail_queue: thumbnail_tx,
            hash_queue: hash_tx,
            hash_result_queue: hash_result_tx,
            hash_result_rx,
            pending_hash_jobs: std::sync::Mutex::new(HashSet::new()),
            thumbnail_inflight: AtomicUsize::new(0),
        });

        let worker_state = Arc::clone(&state);
        let worker_thread = thread::spawn(move || Self::worker_loop(worker_state));

        let watcher_state = Arc::clone(&state);
        let watcher_thread = thread::spawn(move || Self::watcher_loop(watcher_state));

        let hash_worker_count = thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 4);
        let mut hash_threads = Vec::with_capacity(hash_worker_count);
        for _ in 0..hash_worker_count {
            let hash_state = Arc::clone(&state);
            let hash_rx_clone = hash_rx.clone();
            hash_threads.push(thread::spawn(move || Self::hash_worker_loop(hash_state, hash_rx_clone)));
        }

        let thumbnail_state = Arc::clone(&state);
        let thumbnail_thread = thread::spawn(move || Self::thumbnail_worker_loop(thumbnail_state, thumbnail_rx, event_tx));

        Self {
            state,
            watcher_thread: Some(watcher_thread),
            worker_thread: Some(worker_thread),
            thumbnail_thread: Some(thumbnail_thread),
            hash_threads,
        }
    }

    fn scan_library(library_path: &str, db: &Pool<SqliteConnectionManager>, _event_tx: &Sender<LibraryEvent>, state: &Arc<ScannerState>) {
        let mut conn = match db.get() {
            Ok(c) => c,
            Err(e) => {
                log::error!("Scanner: Failed to get connection from pool: {}", e);
                return;
            }
        };

        // Use IMMEDIATE transaction to acquire write lock immediately
        // This prevents race conditions where multiple transactions try to upgrade to exclusive
        let transaction = match conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate) {
            Ok(t) => t,
            Err(e) => {
                log::error!("Scanner: Failed to start transaction: {}", e);
                return;
            }
        };
        let walkdir = WalkDir::new(library_path).follow_links(false).min_depth(1).into_iter().filter_entry(|e| !e.file_name().as_bytes().starts_with(b".")).filter_map(Result::ok);

        let mut uuid_stack: Vec<String> = vec!["__root__".to_string()];
        for entry in walkdir {
            let depth = entry.depth();
            while uuid_stack.len() > depth {
                uuid_stack.pop();
            }
            let parent_uuid = uuid_stack.last().unwrap();

            if entry.file_type().is_dir() {
                let uuid_path = entry.path().join(".biblos_uuid");
                let uuid = match fs::read_to_string(&uuid_path) {
                    Ok(uuid) => uuid,
                    Err(_) => {
                        let uuid = Uuid::new_v4().to_string();
                        fs::write(uuid_path, &uuid).unwrap();
                        uuid
                    }
                };
                upsert_dir(&transaction, &uuid, parent_uuid, entry.file_name().to_str().unwrap());
                uuid_stack.push(uuid);
            } else if entry.file_type().is_file() && entry.path().extension() == Some("epub".as_ref()) {
                let (book_dir, file_hash) = get_book_dir(&transaction, &entry);
                match book_dir {
                    None => {
                        let task = HashTask {
                            parent_uuid: parent_uuid.to_string(),
                            file_name: entry.file_name().to_string_lossy().to_string(),
                            epub_path: entry.path().to_string_lossy().to_string(),
                            file_hash,
                        };
                        let mut pending = state.pending_hash_jobs.lock().unwrap();
                        if pending.insert(file_hash) {
                            let _ = state.hash_queue.send(task);
                        }
                    }
                    Some(book_dir) => {
                        insert_local_book_dir(&transaction, parent_uuid, book_dir.book_id, entry.file_name().to_str().unwrap(), file_hash);
                    }
                }
            }
        }
        match transaction.commit() {
            Ok(_) => (),
            Err(e) => {
                log::error!("Scanner: Failed to commit transaction: {}", e);
                return;
            }
        }
        Self::drain_hash_results_on_scanner_thread(state);
    }
}

impl Drop for LibraryScanner {
    fn drop(&mut self) {
        self.state.should_finish.store(true, Ordering::Release);
        self.state.scan_requested.store(true, Ordering::Release);
        if let Some(thread) = self.state.park_thread.get() {
            thread.unpark();
        }
        // Close the thumbnail queue channel to signal the thumbnail thread to stop
        drop(self.state.thumbnail_queue.clone());
        drop(self.state.hash_queue.clone());
        drop(self.state.hash_result_queue.clone());
        self.watcher_thread.take();
        self.worker_thread.take();
        for thread in self.hash_threads.drain(..) {
            let _ = thread.join();
        }
        self.thumbnail_thread.take();
    }
}

impl LibraryScanner {
    fn hash_worker_loop(state: Arc<ScannerState>, hash_rx: crossbeam_channel::Receiver<HashTask>) {
        let mut book_id_hash_buf = vec![0_u8; 1024 * 1024];
        loop {
            if state.should_finish.load(Ordering::Acquire) {
                break;
            }
            let task = match hash_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(task) => task,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            };

            let book_hash = generate_book_id_blake3(Path::new(&task.epub_path), &mut book_id_hash_buf);
            let (title, author) = match get_epub_id_title_author(Path::new(&task.epub_path)) {
                Some((_, title, author)) => (Some(title), Some(author)),
                None => (None, None),
            };

            let ready = HashReady {
                parent_uuid: task.parent_uuid,
                file_name: task.file_name,
                epub_path: task.epub_path,
                file_hash: task.file_hash,
                book_hash,
                title,
                author,
            };

            if state.hash_result_queue.send(ready).is_err() {
                break;
            }
        }
    }

    fn drain_hash_results_on_scanner_thread(state: &Arc<ScannerState>) {
        const MAX_BATCH: usize = 64;
        loop {
            if state.should_finish.load(Ordering::Acquire) {
                break;
            }

            let pending_len = {
                let pending = state.pending_hash_jobs.lock().unwrap();
                pending.len()
            };
            if pending_len == 0 && state.hash_result_rx.is_empty() {
                break;
            }

            let first = match state.hash_result_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(item) => item,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            };

            let mut batch = vec![first];
            while batch.len() < MAX_BATCH {
                match state.hash_result_rx.try_recv() {
                    Ok(item) => batch.push(item),
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                }
            }

            let mut conn = match state.db.get() {
                Ok(c) => c,
                Err(err) => {
                    log::error!("Scanner inserter: failed to get DB connection: {}", err);
                    let mut pending = state.pending_hash_jobs.lock().unwrap();
                    for item in &batch {
                        pending.remove(&item.file_hash);
                    }
                    continue;
                }
            };

            let tx = match conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate) {
                Ok(tx) => tx,
                Err(err) => {
                    log::error!("Scanner inserter: failed to start transaction: {}", err);
                    let mut pending = state.pending_hash_jobs.lock().unwrap();
                    for item in &batch {
                        pending.remove(&item.file_hash);
                    }
                    continue;
                }
            };

            for item in &batch {
                insert_book(&tx, item.book_hash);
                insert_local_book_dir(&tx, &item.parent_uuid, item.book_hash.to_string(), &item.file_name, item.file_hash);
                if let (Some(title), Some(author)) = (&item.title, &item.author) {
                    update_book_metadata(&tx, item.book_hash, title, author);
                }
            }

            if let Err(err) = tx.commit() {
                log::error!("Scanner inserter: failed to commit transaction: {}", err);
                let mut pending = state.pending_hash_jobs.lock().unwrap();
                for item in &batch {
                    pending.remove(&item.file_hash);
                }
                continue;
            }

            for item in &batch {
                let _ = state.thumbnail_queue.send(ThumbnailTask { book_id: item.book_hash.to_string(), epub_path: item.epub_path.clone() });
                state.thumbnail_inflight.fetch_add(1, Ordering::AcqRel);
            }

            let mut pending = state.pending_hash_jobs.lock().unwrap();
            for item in &batch {
                pending.remove(&item.file_hash);
            }
        }
    }

    fn worker_loop(state: Arc<ScannerState>) {
        let current_thread = thread::current();
        let _ = state.park_thread.set(current_thread);
        let mut last_scan_time: Option<Instant> = None;

        loop {
            if state.should_finish.load(Ordering::Acquire) {
                break;
            }
            if !state.scan_requested.swap(false, Ordering::AcqRel) {
                thread::park();
                continue;
            }

            // Add delay between scans to avoid rapid consecutive scans that could conflict
            if let Some(last_time) = last_scan_time {
                let elapsed = last_time.elapsed();
                if elapsed < Duration::from_millis(500) {
                    thread::sleep(Duration::from_millis(500) - elapsed);
                }
            }

            Self::scan_library(&state.library_path, &state.db, &state.event_tx, &state);
            last_scan_time = Some(Instant::now());
        }
    }

    fn thumbnail_worker_loop(state: Arc<ScannerState>, thumbnail_rx: crossbeam_channel::Receiver<ThumbnailTask>, event_tx: Sender<LibraryEvent>) {
        const BATCH_CAP: usize = 50;
        const FLUSH_INTERVAL: Duration = Duration::from_millis(1000);
        let mut pending_updates: Vec<ThumbnailStateUpdate> = Vec::with_capacity(BATCH_CAP);

        loop {
            match thumbnail_rx.recv_timeout(FLUSH_INTERVAL) {
                Ok(task) => {
                    let thumb_path = get_app_data_thumbnails_dir()
                        .join(format!("{}.jpg", task.book_id))
                        .to_string_lossy()
                        .to_string();
                    let generated_ok = generate_thumbnail_to_app_data(&task.epub_path, &task.book_id).is_ok();
                    let exists = Path::new(&thumb_path).exists();
                    pending_updates.push(ThumbnailStateUpdate {
                        hash: task.book_id.clone(),
                        path: thumb_path,
                        exists: generated_ok && exists,
                    });

                    if generated_ok {
                        let _ = event_tx.send(LibraryEvent::ThumbnailGenerated { book_id: task.book_id });
                    }

                    if pending_updates.len() >= BATCH_CAP {
                        Self::flush_thumbnail_state_updates(&state, &mut pending_updates);
                    }

                    let prev = state.thumbnail_inflight.fetch_sub(1, Ordering::AcqRel);
                    if prev <= 1 {
                        // Drained signal for current scan wave.
                        Self::flush_thumbnail_state_updates(&state, &mut pending_updates);
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if !pending_updates.is_empty() {
                        Self::flush_thumbnail_state_updates(&state, &mut pending_updates);
                    }
                    if state.should_finish.load(Ordering::Acquire) {
                        break;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }

        if !pending_updates.is_empty() {
            Self::flush_thumbnail_state_updates(&state, &mut pending_updates);
        }
    }

    fn flush_thumbnail_state_updates(state: &Arc<ScannerState>, updates: &mut Vec<ThumbnailStateUpdate>) {
        if updates.is_empty() {
            return;
        }
        let mut conn = match state.db.get() {
            Ok(c) => c,
            Err(err) => {
                log::error!("Thumbnail flush: failed to get DB connection: {}", err);
                return;
            }
        };
        let tx = match conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate) {
            Ok(tx) => tx,
            Err(err) => {
                log::error!("Thumbnail flush: failed to start transaction: {}", err);
                return;
            }
        };

        for update in updates.iter() {
            upsert_blob_local_ref_tx(&tx, "thumbnail", &update.hash, &update.path, update.exists);
            derive_blob_state_from_local_tx(&tx, "thumbnail", &update.hash);
        }

        if let Err(err) = tx.commit() {
            log::error!("Thumbnail flush: failed to commit transaction: {}", err);
            return;
        }
        updates.clear();
    }

    fn watcher_loop(state: Arc<ScannerState>) {
        let (event_tx, event_rx) = std_channel();
        let mut watcher = recommended_watcher(event_tx).expect("Failed to start filesystem watcher");
        watcher.watch(Path::new(&state.library_path), RecursiveMode::Recursive).expect("Failed to watch library path");
        let debounce = Duration::from_millis(500);
        let mut pending_scan = false;
        let mut last_event = None;
        loop {
            if state.should_finish.load(Ordering::Acquire) {
                break;
            }
            match event_rx.recv_timeout(debounce) {
                Ok(Ok(event)) => {
                    if Self::should_trigger_scan(&event) {
                        pending_scan = true;
                        last_event = Some(Instant::now());
                    }
                }
                Ok(Err(e)) => log::error!("Watch error: {:?}", e),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if pending_scan {
                        if let Some(last_event) = last_event {
                            if last_event.elapsed() >= debounce {
                                Self::request_scan(&state);
                                pending_scan = false;
                            }
                        }
                    }
                }
                Err(e) => {
                    log::error!("Channel error: {:?}", e);
                    break;
                }
            }
        }
        drop(watcher);
    }

    fn should_trigger_scan(event: &Event) -> bool {
        if event.paths.is_empty() {
            return true;
        }
        event.paths.iter().any(|path| Self::is_relevant_path(path))
    }

    fn request_scan(state: &ScannerState) {
        state.scan_requested.store(true, Ordering::Release);
        if let Some(thread) = state.park_thread.get() {
            thread.unpark();
        }
    }

    fn is_relevant_path(path: &Path) -> bool {
        if path.components().any(|c| c.as_os_str() == ".biblos") {
            return false;
        }
        if path.is_dir() {
            return true;
        }
        Self::is_epub_file(path)
    }

    fn is_epub_file(path: &Path) -> bool {
        path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.eq_ignore_ascii_case("epub")).unwrap_or(false)
    }
}

fn get_book_dir(transaction: &Transaction, entry: &DirEntry) -> (Option<BookDir>, u64) {
    let mut hasher = Xxh3::new();
    let metadata = entry.metadata().unwrap();
    let modified_time = metadata.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_secs();
    hasher.write_u64(modified_time);
    hasher.write_u64(metadata.size());
    hasher.write_u64(metadata.ino());
    hasher.write_u64(metadata.dev());
    let hash = hasher.digest();
    (get_book_dir_by_hash(transaction, hash), hash)
}

use crossbeam_channel::Sender;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Transaction;
use uuid::Uuid;
use xxhash_rust::xxh3::Xxh3;
