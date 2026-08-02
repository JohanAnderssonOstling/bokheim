use crate::db;
use crate::db::{get_book_dir_by_hash, insert_book, insert_local_book_dir, update_book_metadata, upsert_dir};
use crate::epub::get_epub_id_title_author;
use crate::home::BackendContext;
use crate::library::generate_content_hash_blake3;
use crate::library::generate_thumbnail_for_sync;
use crate::path_types::LibraryRoot;
#[cfg(test)]
use crate::APP_HIDDEN_LIBRARY_DB_PATH;
use crate::{BookId, ContentHash, APP_HIDDEN_DIR};
use notify::{recommended_watcher, Event, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::HashSet;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel as std_channel;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use std::time::Instant;
use std::{fs, thread};
use walkdir::WalkDir;

/// Delay before re-scanning to advance the vanish grace count for files missing
/// but not yet past the window. Sets how fast a real deletion propagates and how
/// long a transient miss is tolerated.
const RESCAN_DELAY: Duration = Duration::from_secs(5);

pub use crate::DownloadState;

#[derive(Debug, Clone, Serialize)]
pub enum LibraryEvent {
    BookAdded { book_id: BookId, path: String },
    BookRemoved { book_id: BookId, path: String },
    DirectoryAdded { path: String },
    DirectoryRemoved { path: String },
    ThumbnailGenerated { book_id: BookId },
    DownloadStatusChanged { book_id: BookId, state: DownloadState },
    FolderDownloadStatusChanged { dir_id: crate::DirId, downloaded_books: usize, total_books: usize },
    BookOpened { book_id: BookId },
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
    context: BackendContext,
    library_root: LibraryRoot,
    db: Pool<SqliteConnectionManager>,
    park_thread: OnceLock<thread::Thread>,
    scan_requested: AtomicBool,
    should_finish: AtomicBool,
    thumbnail_queue: crossbeam_channel::Sender<ThumbnailTask>,
    hash_queue: crossbeam_channel::Sender<HashTask>,
    hash_result_queue: crossbeam_channel::Sender<HashResult>,
    hash_result_rx: crossbeam_channel::Receiver<HashResult>,
    pending_hash_jobs: std::sync::Mutex<HashSet<ScanFileKey>>,
    thumbnail_inflight: AtomicUsize,
}

struct ThumbnailTask {
    book_id: BookId,
    content_hash: ContentHash,
    book_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ScanFileKey {
    dir_id: sync_common::DirId,
    file_name: sync_common::FileName,
    rel_path: crate::RelativeBookPath,
    fingerprint: u64,
}

#[derive(Clone)]
struct HashTask {
    key: ScanFileKey,
    book_path: PathBuf,
}

struct BookMetadata {
    title: String,
    author: String,
}

enum HashResult {
    Ready { task: HashTask, book_id: BookId, content_hash: ContentHash, metadata: Option<BookMetadata> },
    Failed { task: HashTask, error: String },
}

impl HashResult {
    fn key(&self) -> &ScanFileKey {
        match self {
            Self::Ready { task, .. } | Self::Failed { task, .. } => &task.key,
        }
    }
}

impl LibraryScanner {
    pub fn new(context: BackendContext, library_path: String, db: Pool<SqliteConnectionManager>, event_tx: Sender<LibraryEvent>) -> std::io::Result<Self> {
        let library_root = LibraryRoot::open(Path::new(&library_path))?;
        Self::from_root(context, library_root, db, event_tx)
    }

    pub(crate) fn from_root(context: BackendContext, library_root: LibraryRoot, db: Pool<SqliteConnectionManager>, event_tx: Sender<LibraryEvent>) -> std::io::Result<Self> {
        let (thumbnail_tx, thumbnail_rx) = crossbeam_channel::unbounded::<ThumbnailTask>();
        let (hash_tx, hash_rx) = crossbeam_channel::unbounded::<HashTask>();
        let (hash_result_tx, hash_result_rx) = crossbeam_channel::unbounded::<HashResult>();

        let state = Arc::new(ScannerState {
            context,
            library_root,
            db,
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

        Ok(Self { state, watcher_thread: Some(watcher_thread), worker_thread: Some(worker_thread), thumbnail_thread: Some(thumbnail_thread), hash_threads })
    }

    fn scan_library(db: &Pool<SqliteConnectionManager>, state: &Arc<ScannerState>) {
        let mut conn = match db.get() {
            Ok(c) => c,
            Err(e) => {
                log::error!("Scanner: Failed to get connection from pool: {}", e);
                return;
            }
        };

        // Filesystem effects committed by sync are durable outbox work. Drain
        // them before walking the disk, then hide any transient failures from
        // the scanner so stale files cannot be published as fresh adds.
        if let Err(error) = db::drain_pending_fs_operations(&conn, state.library_root.as_path()) {
            log::error!("Scanner: pending filesystem operations did not drain safely: {error}");
            return;
        }
        let pending_removals = match db::pending_removal_paths(&conn) {
            Ok(paths) => paths,
            Err(error) => {
                log::error!("Scanner: failed to load pending filesystem paths: {error}");
                return;
            }
        };

        // Claim this scan's generation up front so both the synchronous walk and
        // the async hash drain stamp the same value; placements left unstamped
        // afterward are reconciled as vanished.
        let scan_id = match db::next_scan_id(&conn) {
            Ok(scan_id) => scan_id,
            Err(error) => {
                log::error!("Scanner: failed to allocate scan generation: {error}");
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
        let library_root = state.library_root.as_path().to_path_buf();
        let pending_files = pending_removals.files;
        let pending_dirs = pending_removals.dirs;
        let mut walkdir = WalkDir::new(state.library_root.as_path()).follow_links(false).min_depth(1).into_iter();
        let root_dir_id = sync_common::DirId::parse("__root__").expect("the root directory id is valid");
        let mut uuid_stack = vec![root_dir_id.clone()];
        let mut seen_dir_ids = HashSet::from([root_dir_id]);
        let mut book_files_seen: usize = 0;
        while let Some(next_entry) = walkdir.next() {
            let entry = match next_entry {
                Ok(entry) => entry,
                Err(error) => {
                    log::warn!("Scanner: skipping unreadable entry: {error}");
                    continue;
                }
            };
            if entry.file_name().as_bytes().starts_with(b".") {
                if entry.file_type().is_dir() {
                    walkdir.skip_current_dir();
                }
                continue;
            }
            let relative = match entry.path().strip_prefix(&library_root) {
                Ok(relative) => relative,
                Err(error) => {
                    log::warn!("Scanner: path escaped library root: {error}");
                    if entry.file_type().is_dir() {
                        walkdir.skip_current_dir();
                    }
                    continue;
                }
            };
            if pending_files.contains(relative) || pending_dirs.iter().any(|dir| relative == dir || relative.starts_with(dir)) {
                if entry.file_type().is_dir() {
                    walkdir.skip_current_dir();
                }
                continue;
            }
            let depth = entry.depth();
            while uuid_stack.len() > depth {
                uuid_stack.pop();
            }
            let Some(parent_dir_id) = uuid_stack.last().cloned() else {
                log::error!("Scanner: directory identity stack was empty at {}", entry.path().display());
                return;
            };

            if entry.file_type().is_dir() {
                let Some(raw_name) = entry.file_name().to_str() else {
                    log::warn!("Scanner: skipping directory with a non-UTF-8 name: {}", entry.path().display());
                    walkdir.skip_current_dir();
                    continue;
                };
                let directory_name = match sync_common::FileName::parse(raw_name) {
                    Ok(name) => name,
                    Err(error) => {
                        log::warn!("Scanner: skipping invalid directory name '{}': {error}", entry.path().display());
                        walkdir.skip_current_dir();
                        continue;
                    }
                };
                let uuid_path = entry.path().join(".biblos_uuid");
                let mut dir_id = match fs::read_to_string(&uuid_path) {
                    Ok(uuid) => match sync_common::DirId::parse(uuid.trim()) {
                        Ok(dir_id) => dir_id,
                        Err(error) => {
                            log::warn!("Scanner: skipping directory with invalid marker {}: {error}", uuid_path.display());
                            walkdir.skip_current_dir();
                            continue;
                        }
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        let dir_id = sync_common::DirId::parse(&Uuid::new_v4().to_string()).expect("UUID is a valid directory id");
                        if let Err(error) = fs::write(&uuid_path, dir_id.as_str()) {
                            log::error!("Scanner: cannot create directory marker {}: {error}", uuid_path.display());
                            walkdir.skip_current_dir();
                            continue;
                        }
                        dir_id
                    }
                    Err(error) => {
                        log::warn!("Scanner: cannot read directory marker {}: {error}", uuid_path.display());
                        walkdir.skip_current_dir();
                        continue;
                    }
                };
                if !seen_dir_ids.insert(dir_id.clone()) {
                    dir_id = sync_common::DirId::parse(&Uuid::new_v4().to_string()).expect("UUID is a valid directory id");
                    if let Err(error) = fs::write(&uuid_path, dir_id.as_str()) {
                        log::error!("Scanner: cannot replace duplicate directory marker {}: {error}", uuid_path.display());
                        walkdir.skip_current_dir();
                        continue;
                    }
                    seen_dir_ids.insert(dir_id.clone());
                }
                if let Err(error) = upsert_dir(&transaction, &dir_id, &parent_dir_id, &directory_name) {
                    log::error!("Scanner: failed to persist directory {}: {error}", entry.path().display());
                    return;
                }
                uuid_stack.push(dir_id);
            } else if entry.file_type().is_file() && Self::is_supported_book_file(entry.path()) {
                let Some(raw_file_name) = entry.file_name().to_str() else {
                    log::warn!("Scanner: skipping EPUB with a non-UTF-8 name: {}", entry.path().display());
                    continue;
                };
                let file_name = match sync_common::FileName::parse(raw_file_name) {
                    Ok(file_name) => file_name,
                    Err(error) => {
                        log::warn!("Scanner: skipping invalid EPUB filename '{}': {error}", entry.path().display());
                        continue;
                    }
                };
                let relative_path = match relative.to_str().and_then(|path| crate::RelativeBookPath::parse(path).ok()) {
                    Some(relative_path) => relative_path,
                    None => {
                        log::warn!("Scanner: skipping EPUB with an invalid relative path: {}", entry.path().display());
                        continue;
                    }
                };
                match db::is_book_file_pending_removal_tx(&transaction, parent_dir_id.as_str(), file_name.as_str()) {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(error) => {
                        log::error!("Scanner: failed to check pending removal state: {error}");
                        return;
                    }
                }
                book_files_seen += 1;
                let file_hash = match db::local_file_fingerprint(entry.path()) {
                    Ok(file_hash) => file_hash,
                    Err(error) => {
                        log::warn!("Scanner: cannot fingerprint {}: {error}", entry.path().display());
                        continue;
                    }
                };
                let book_dir = match get_book_dir_by_hash(&transaction, file_hash) {
                    Ok(book_dir) => book_dir,
                    Err(error) => {
                        log::error!("Scanner: failed to look up fingerprint for {}: {error}", entry.path().display());
                        return;
                    }
                };
                match book_dir {
                    // Known and already carries its content hash: re-affirm presence
                    // without re-reading the file.
                    Some(book_dir) => {
                        if let Err(error) = insert_local_book_dir(&transaction, &parent_dir_id, book_dir.book_id, &file_name, file_hash, &book_dir.content_hash, scan_id) {
                            log::error!("Scanner: failed to persist placement {}: {error}", entry.path().display());
                            return;
                        }
                    }
                    // Unknown file, or a known row from before the content_hash column
                    // existed: hash the contents to (back)fill the portable blob key.
                    _ => {
                        let key = ScanFileKey { dir_id: parent_dir_id, file_name, rel_path: relative_path, fingerprint: file_hash };
                        let task = HashTask { key: key.clone(), book_path: entry.path().to_path_buf() };
                        let mut pending = state.pending_hash_jobs.lock().unwrap();
                        if pending.insert(key.clone()) && state.hash_queue.send(task).is_err() {
                            pending.remove(&key);
                        }
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
        Self::drain_hash_results_on_scanner_thread(state, scan_id);

        // Reconcile vanished files now that every present placement (sync walk +
        // async hashing) has been stamped with this scan's generation. Removals
        // propagate to other devices (deleting their copies), so guard against a
        // scan that found nothing — an unmounted or transiently-empty library
        // root must not mass-delete every book everywhere. A genuinely empty
        // library has no local placements to reconcile, so skipping is harmless.
        if book_files_seen > 0 {
            if let Ok(conn) = state.db.get() {
                if let Err(error) = db::mark_vanished_book_dirs_deleted(&conn, scan_id) {
                    log::error!("Scanner: failed to reconcile vanished placements: {error}");
                    return;
                }
                // Files missing but still inside the grace window need another scan
                // to cross the threshold. A removal only triggers one scan, so
                // schedule a follow-up rather than waiting on further FS events.
                if db::count_pending_vanish_candidates(&conn, scan_id).map_or_else(
                    |error| {
                        log::error!("Scanner: failed to count pending vanished placements: {error}");
                        false
                    },
                    |count| count > 0,
                ) {
                    Self::schedule_rescan(state, RESCAN_DELAY);
                }
            }
        } else {
            log::warn!("Scan found no supported book files; skipping vanished-file reconciliation (library may be unmounted)");
        }
    }

    /// Requests another scan after `delay` so grace-windowed vanish candidates get
    /// re-checked even when no filesystem event would otherwise trigger one.
    fn schedule_rescan(state: &Arc<ScannerState>, delay: Duration) {
        let state = Arc::clone(state);
        thread::spawn(move || {
            thread::sleep(delay);
            if !state.should_finish.load(Ordering::Acquire) {
                Self::request_scan(&state);
            }
        });
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

            let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let validated = book_format::validate_path(&task.book_path).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
                if Self::book_format_from_path(&task.book_path) != Some(validated.format) {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "book extension does not match its contents"));
                }
                let (book_id, content_hash) = generate_content_hash_blake3(&task.book_path, &mut book_id_hash_buf)?;
                let metadata = match validated.format {
                    book_format::BookFormat::Epub => match get_epub_id_title_author(&task.book_path) {
                        Ok((_, title, author)) => Some(BookMetadata { title, author }),
                        Err(error) => {
                            log::warn!("Skipping EPUB metadata read for {}: {error}", task.book_path.display());
                            None
                        }
                    },
                    book_format::BookFormat::Pdf => Some(BookMetadata { title: task.book_path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("Untitled").to_owned(), author: String::new() }),
                };
                Ok::<_, std::io::Error>((book_id, content_hash, metadata))
            })) {
                Ok(Ok((book_id, content_hash, metadata))) => HashResult::Ready { task, book_id, content_hash, metadata },
                Ok(Err(error)) => HashResult::Failed { task, error: error.to_string() },
                Err(error) => {
                    let message = error.downcast_ref::<&str>().copied().or_else(|| error.downcast_ref::<String>().map(String::as_str)).unwrap_or("unknown panic");
                    HashResult::Failed { task, error: format!("hash worker panicked: {message}") }
                }
            };
            let key = result.key().clone();
            if state.hash_result_queue.send(result).is_err() {
                state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&key);
                break;
            }
        }
    }

    fn drain_hash_results_on_scanner_thread(state: &Arc<ScannerState>, scan_id: i64) {
        const MAX_BATCH: usize = 64;
        const STALLED_JOB_TIMEOUT: Duration = Duration::from_secs(30);
        let mut last_result_at = Instant::now();
        loop {
            if state.should_finish.load(Ordering::Acquire) {
                break;
            }

            let pending_len = {
                let pending = state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                pending.len()
            };
            if pending_len == 0 && state.hash_result_rx.is_empty() {
                break;
            }

            let first = match state.hash_result_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(item) => {
                    last_result_at = Instant::now();
                    item
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) if last_result_at.elapsed() < STALLED_JOB_TIMEOUT => continue,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    let mut pending = state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    log::error!("Scanner: abandoning {} stalled hash jobs; the next scan will retry them", pending.len());
                    pending.clear();
                    break;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
                    break;
                }
            };

            let mut batch = vec![first];
            while batch.len() < MAX_BATCH {
                match state.hash_result_rx.try_recv() {
                    Ok(item) => batch.push(item),
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                }
            }

            for item in &batch {
                if let HashResult::Failed { task, error } = item {
                    log::warn!("Scanner: failed to hash {}: {error}", task.book_path.display());
                }
            }
            if batch.iter().all(|item| matches!(item, HashResult::Failed { .. })) {
                let mut pending = state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                for item in &batch {
                    pending.remove(item.key());
                }
                continue;
            }

            let mut conn = match state.db.get() {
                Ok(c) => c,
                Err(err) => {
                    log::error!("Scanner inserter: failed to get DB connection: {}", err);
                    let mut pending = state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    for item in &batch {
                        pending.remove(item.key());
                    }
                    continue;
                }
            };

            let tx = match conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate) {
                Ok(tx) => tx,
                Err(err) => {
                    log::error!("Scanner inserter: failed to start transaction: {}", err);
                    let mut pending = state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    for item in &batch {
                        pending.remove(item.key());
                    }
                    continue;
                }
            };

            let mut batch_db_error = None;
            for item in &batch {
                let HashResult::Ready { task, book_id, content_hash, metadata, .. } = item else {
                    continue;
                };
                match db::is_book_file_pending_removal_tx(&tx, task.key.dir_id.as_str(), task.key.file_name.as_str()) {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(error) => {
                        batch_db_error = Some(error);
                        break;
                    }
                }
                if let Err(error) = insert_book(&tx, *book_id) {
                    batch_db_error = Some(error);
                    break;
                }
                if let Err(error) = insert_local_book_dir(&tx, &task.key.dir_id, *book_id, &task.key.file_name, task.key.fingerprint, content_hash, scan_id) {
                    batch_db_error = Some(error);
                    break;
                }
                if let Some(metadata) = metadata {
                    if let Err(error) = update_book_metadata(&tx, *book_id, &metadata.title, &metadata.author) {
                        batch_db_error = Some(error);
                        break;
                    }
                }
            }

            if let Some(error) = batch_db_error {
                log::error!("Scanner inserter: database batch failed: {error}");
                drop(tx);
                let mut pending = state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                for item in &batch {
                    pending.remove(item.key());
                }
                continue;
            }

            if let Err(err) = tx.commit() {
                log::error!("Scanner inserter: failed to commit transaction: {}", err);
                let mut pending = state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                for item in &batch {
                    pending.remove(item.key());
                }
                continue;
            }

            for item in &batch {
                if let HashResult::Ready { task, book_id, content_hash, .. } = item {
                    if state.thumbnail_queue.send(ThumbnailTask { book_id: *book_id, content_hash: content_hash.clone(), book_path: task.book_path.clone() }).is_ok() {
                        state.thumbnail_inflight.fetch_add(1, Ordering::AcqRel);
                    }
                }
            }

            let mut pending = state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            for item in &batch {
                pending.remove(item.key());
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

            Self::scan_library(&state.db, &state);
            last_scan_time = Some(Instant::now());
        }
    }

    fn thumbnail_worker_loop(state: Arc<ScannerState>, thumbnail_rx: crossbeam_channel::Receiver<ThumbnailTask>, event_tx: Sender<LibraryEvent>) {
        const FLUSH_INTERVAL: Duration = Duration::from_millis(1000);

        loop {
            match thumbnail_rx.recv_timeout(FLUSH_INTERVAL) {
                Ok(task) => {
                    let generated_ok = generate_thumbnail_for_sync(&state.context, &state.library_root, &task.book_path, task.book_id, &task.content_hash).is_ok();

                    if generated_ok {
                        let _ = event_tx.send(LibraryEvent::ThumbnailGenerated { book_id: task.book_id });
                    }

                    state.thumbnail_inflight.fetch_sub(1, Ordering::AcqRel);
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if state.should_finish.load(Ordering::Acquire) {
                        break;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    fn watcher_loop(state: Arc<ScannerState>) {
        let (event_tx, event_rx) = std_channel();
        let mut watcher = match recommended_watcher(event_tx) {
            Ok(watcher) => watcher,
            Err(error) => {
                log::error!("Failed to start filesystem watcher: {error}");
                return;
            }
        };
        if let Err(error) = watcher.watch(state.library_root.as_path(), RecursiveMode::Recursive) {
            log::error!("Failed to watch library path {}: {error}", state.library_root.as_path().display());
            return;
        }
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
        if path.components().any(|c| c.as_os_str() == APP_HIDDEN_DIR) {
            return false;
        }
        if path.is_dir() {
            return true;
        }
        Self::is_supported_book_file(path)
    }

    fn is_supported_book_file(path: &Path) -> bool {
        Self::book_format_from_path(path).is_some()
    }

    fn book_format_from_path(path: &Path) -> Option<book_format::BookFormat> {
        let extension = path.extension()?.to_str()?;
        if extension.eq_ignore_ascii_case("epub") {
            Some(book_format::BookFormat::Epub)
        } else if extension.eq_ignore_ascii_case("pdf") {
            Some(book_format::BookFormat::Pdf)
        } else {
            None
        }
    }
}

use crossbeam_channel::Sender;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use uuid::Uuid;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db,
        home::{AppDataLocation, BackendContext},
        RelativeBookPath,
    };
    use r2d2::Pool;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::{fs, thread, time::Duration};
    use sync_common::{DirId, FileName};
    use tempfile::tempdir;

    fn test_scanner_state(context: &BackendContext, library_root: &LibraryRoot) -> (Arc<ScannerState>, crossbeam_channel::Receiver<HashTask>) {
        let db_path = library_root.as_path().join(APP_HIDDEN_LIBRARY_DB_PATH);
        fs::create_dir_all(db_path.parent().expect("library db parent exists")).unwrap();

        let manager = SqliteConnectionManager::file(db_path);
        let db = Pool::builder().max_size(4).connection_timeout(Duration::from_secs(30)).build(manager).unwrap();
        {
            let conn = db.get().unwrap();
            db::init_db(&conn).unwrap();
        }

        let (thumbnail_tx, _thumbnail_rx) = crossbeam_channel::unbounded::<ThumbnailTask>();
        let (hash_tx, hash_rx) = crossbeam_channel::unbounded::<HashTask>();
        let (hash_result_tx, hash_result_rx) = crossbeam_channel::unbounded::<HashResult>();

        let state = Arc::new(ScannerState {
            context: context.clone(),
            library_root: library_root.clone(),
            db,
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

        (state, hash_rx)
    }

    fn count_dir_state_events(conn: &rusqlite::Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM event_log WHERE event_type='dir_state' AND json_extract(payload, '$.deleted') = 0", (), |row| row.get(0)).unwrap()
    }

    fn dir_count(conn: &rusqlite::Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM dir WHERE id != '__root__' AND deleted_at IS NULL", (), |row| row.get(0)).unwrap()
    }

    #[test]
    fn pending_hash_identity_includes_placement_not_only_fingerprint() {
        let first = ScanFileKey { dir_id: DirId::parse("first").unwrap(), file_name: FileName::parse("book.epub").unwrap(), rel_path: RelativeBookPath::parse("First/book.epub").unwrap(), fingerprint: 7 };
        let second = ScanFileKey { dir_id: DirId::parse("second").unwrap(), file_name: FileName::parse("book.epub").unwrap(), rel_path: RelativeBookPath::parse("Second/book.epub").unwrap(), fingerprint: 7 };
        let mut pending = HashSet::new();
        assert!(pending.insert(first));
        assert!(pending.insert(second));
    }

    #[test]
    fn hash_worker_missing_file_emits_failure_and_clears_pending() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let library_root = LibraryRoot::open(&library_root_path).unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().to_path_buf())).unwrap();
        let (state, hash_rx) = test_scanner_state(&context, &library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));

        let key = ScanFileKey { dir_id: DirId::parse("dir-a").unwrap(), file_name: FileName::parse("missing.epub").unwrap(), rel_path: RelativeBookPath::parse("missing.epub").unwrap(), fingerprint: 7 };
        state.pending_hash_jobs.lock().unwrap().insert(key.clone());
        let task = HashTask { key: key.clone(), book_path: library_root_path.join("missing.epub") };
        state.hash_queue.send(task).unwrap();

        LibraryScanner::drain_hash_results_on_scanner_thread(&state, 1);

        assert!(state.pending_hash_jobs.lock().unwrap().is_empty());

        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
    }

    #[test]
    fn duplicate_markers_are_rewritten_and_directory_ids_stay_stable() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let library_root = LibraryRoot::open(&library_root_path).unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().to_path_buf())).unwrap();

        let left = library_root_path.join("First");
        let right = library_root_path.join("Second");
        fs::create_dir_all(&left).unwrap();
        fs::create_dir_all(&right).unwrap();
        fs::write(left.join(".biblos_uuid"), "shared-uuid").unwrap();
        fs::write(right.join(".biblos_uuid"), "shared-uuid").unwrap();
        fs::write(left.join("first.epub"), b"first bytes").unwrap();
        fs::write(right.join("second.epub"), b"second bytes").unwrap();

        let (state, hash_rx) = test_scanner_state(&context, &library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));

        LibraryScanner::scan_library(&state.db, &state);

        let first_marker_after_first = fs::read_to_string(left.join(".biblos_uuid")).unwrap();
        let second_marker_after_first = fs::read_to_string(right.join(".biblos_uuid")).unwrap();
        assert_ne!(first_marker_after_first, second_marker_after_first);
        assert_ne!(first_marker_after_first.trim(), "");
        assert_ne!(second_marker_after_first.trim(), "");

        let conn_first_scan = state.db.get().unwrap();
        assert_eq!(dir_count(&conn_first_scan), 2);
        let first_scan_dir_events = count_dir_state_events(&conn_first_scan);

        LibraryScanner::scan_library(&state.db, &state);
        let first_marker_after_second = fs::read_to_string(left.join(".biblos_uuid")).unwrap();
        let second_marker_after_second = fs::read_to_string(right.join(".biblos_uuid")).unwrap();
        assert_eq!(first_marker_after_first, first_marker_after_second);
        assert_eq!(second_marker_after_first, second_marker_after_second);

        let conn_second_scan = state.db.get().unwrap();
        let second_scan_dir_events = count_dir_state_events(&conn_second_scan);
        assert_eq!(first_scan_dir_events, second_scan_dir_events);

        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();

        // Ensure the two duplicate markers ended up as distinct directory identities.
        let mut remaining = HashSet::new();
        remaining.insert(first_marker_after_first);
        remaining.insert(second_marker_after_first);
        assert_eq!(remaining.len(), 2);
    }
}
