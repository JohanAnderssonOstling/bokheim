use crate::db::{get_book_dir_by_hash, get_books_without_title, insert_book, insert_local_book_dir, update_book_title, upsert_dir, BookDir};
use crate::epub::get_epub_id_and_title;
use notify::{recommended_watcher, Event, RecursiveMode, Watcher};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel as std_channel;
use std::sync::{Arc, OnceLock};
use std::{fs, thread};
use std::hash::Hasher;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, UNIX_EPOCH};
use std::time::Instant;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Clone, Serialize)]
pub enum DownloadState {
    NotDownloaded,
    Downloading(f32),
    Downloaded,
}

#[derive(Debug, Clone, Serialize)]
pub enum LibraryEvent {
    BookAdded {
        book_id: String,
        path: String,
    },
    BookRemoved {
        book_id: String,
        path: String,
    },
    DirectoryAdded {
        path: String,
    },
    DirectoryRemoved {
        path: String,
    },
    ThumbnailGenerated {
        book_id: String,
    },
    DownloadStatusChanged {
        book_id: String,
        state: DownloadState,
    },
    ScanComplete,
}

pub struct LibraryScanner {
    state: Arc<ScannerState>,
    watcher_thread: Option<thread::JoinHandle<()>>,
    worker_thread: Option<thread::JoinHandle<()>>,
}

struct ScannerState {
    library_path: String,
    db: Pool<SqliteConnectionManager>,
    event_tx: Sender<LibraryEvent>,
    park_thread: OnceLock<thread::Thread>,
    scan_requested: AtomicBool,
    should_finish: AtomicBool,
}

impl LibraryScanner {
    pub fn new(library_path: String, db: Pool<SqliteConnectionManager>, event_tx: Sender<LibraryEvent>) -> Self {
        let state = Arc::new(ScannerState {
            library_path,
            db,
            event_tx,
            park_thread: OnceLock::new(),
            scan_requested: AtomicBool::new(true),
            should_finish: AtomicBool::new(false),
        });

        let worker_state = Arc::clone(&state);
        let worker_thread = thread::spawn(move || Self::worker_loop(worker_state));

        let watcher_state = Arc::clone(&state);
        let watcher_thread = thread::spawn(move || Self::watcher_loop(watcher_state));

        Self {
            state,
            watcher_thread: Some(watcher_thread),
            worker_thread: Some(worker_thread),
        }
    }

    fn scan_library(library_path: &str, db: &Pool<SqliteConnectionManager>, event_tx: &Sender<LibraryEvent>) {
        let mut conn = db.get().unwrap();
        let transaction = conn.transaction().unwrap();
        let now = Instant::now();
        let walkdir = WalkDir::new(library_path)
            .follow_links(false)
            .min_depth(1)
            .into_iter()
            .filter_entry(|e| !e.file_name().as_bytes().starts_with(b"."))
            .filter_map(Result::ok);


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
                    Ok(uuid) => {uuid}
                    Err(_) => {
                        let uuid = Uuid::new_v4().to_string();
                        fs::write(uuid_path, &uuid).unwrap();
                        uuid
                    }
                };
                upsert_dir(&transaction, &uuid, parent_uuid, entry.file_name().to_str().unwrap());
                uuid_stack.push(uuid);
            }
            else if entry.file_type().is_file() && entry.path().extension() == Some("epub".as_ref()) {
                let (book_dir, hash) = get_book_dir(&transaction, &entry);
                match book_dir {
                    None => {
                        let book_hash = generate_file_hash(entry.path());
                        insert_book(&transaction, book_hash);
                        insert_local_book_dir(
                            &transaction,
                            parent_uuid,
                            book_hash,
                            entry.file_name().to_str().unwrap(),
                            hash,
                        );
                    }
                    Some(book_dir) => {
                        let file_name = entry.file_name().to_str().unwrap();
                        transaction
                            .insert_book_dir(
                                parent_uuid,
                                &book_dir.book_id,
                                file_name,
                                &hash.to_string(),
                                1,
                            )
                            .unwrap();
                    }
                }
            }
        }
        let transaction_time = Instant::now();
        if transaction.commit().is_err() {
            todo!()
        }
        println!("Transaction time: {}", transaction_time.elapsed().as_millis());
        println!("Scan time: {:?}", now.elapsed().as_millis());
        let mut conn = db.get().unwrap();
        let title_pass_transaction = conn.transaction().unwrap();
        let new_books = get_books_without_title(&title_pass_transaction);
        let library_path = PathBuf::from(library_path);
        let now = Instant::now();
        for book in new_books {
            let (book_id, relative_book_path) = book;
            let absolute_book_path = library_path.join(relative_book_path);
            println!("Absolute path: {:#?}", absolute_book_path);
            let (_, title) = get_epub_id_and_title(&absolute_book_path).unwrap();
            update_book_title(&title_pass_transaction, book_id, &title);
        }
        title_pass_transaction.commit().unwrap();
        println!("Title updating time: {:?}", now.elapsed().as_millis());

    }

}

impl Drop for LibraryScanner {
    fn drop(&mut self) {
        self.state.should_finish.store(true, Ordering::Release);
        self.state.scan_requested.store(true, Ordering::Release);
        if let Some(thread) = self.state.park_thread.get() {
            thread.unpark();
        }
        self.watcher_thread.take();
        self.worker_thread.take();
    }
}

impl LibraryScanner {
    fn worker_loop(state: Arc<ScannerState>) {
        let current_thread = thread::current();
        let _ = state.park_thread.set(current_thread);
        loop {
            if state.should_finish.load(Ordering::Acquire) {
                break;
            }
            if !state.scan_requested.swap(false, Ordering::AcqRel) {
                thread::park();
                continue;
            }
            Self::scan_library(&state.library_path, &state.db, &state.event_tx);
        }
    }

    fn watcher_loop(state: Arc<ScannerState>) {
        let (event_tx, event_rx) = std_channel();
        let mut watcher = recommended_watcher(event_tx).expect("Failed to start filesystem watcher");
        watcher
            .watch(Path::new(&state.library_path), RecursiveMode::Recursive)
            .expect("Failed to watch library path");
        log::info!("Filesystem watcher started on {}", state.library_path);
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
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("epub"))
            .unwrap_or(false)
    }
}

fn get_book_dir(transaction: &Transaction, entry: &DirEntry) -> (Option<BookDir>, u64) {
    let mut hasher      = Xxh3::new();
    let metadata        = entry.metadata().unwrap();
    let modified_time   = metadata.modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_secs();
    hasher.write_u64(modified_time);
    hasher.write_u64(metadata.size());
    hasher.write_u64(metadata.ino());
    hasher.write_u64(metadata.dev());
    let hash            = hasher.digest();
    (get_book_dir_by_hash(transaction, hash), hash)
}

use crossbeam_channel::Sender;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Transaction;
use uuid::Uuid;
use xxhash_rust::xxh3::Xxh3;
use crate::library::generate_file_hash;
