use crate::app::BackendContext;
use crate::library::database as db;
use crate::library::database::{get_book_dirs_by_local_fingerprint, get_tombstoned_book_file_keys, insert_book, insert_local_book_dir, mark_known_book_dirs_seen, upsert_dir};
use crate::library::generate_content_hash_blake3;
use crate::library::generate_thumbnail_for_sync;
use crate::library::thumbnail_needs_repair;
#[cfg(test)]
use crate::APP_HIDDEN_LIBRARY_DB_PATH;
use crate::{ContentHash, DirId, APP_HIDDEN_DIR};
use library_model::BookFormat;
#[cfg(feature = "watcher")]
use notify::{recommended_watcher, Event, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "watcher")]
use std::sync::mpsc::channel as std_channel;
use std::sync::{Arc, Condvar, OnceLock};
use std::time::Duration;
use std::time::Instant;
use std::{fs, thread};
use walkdir::WalkDir;

/// Delay before re-scanning to advance the vanish grace count for files missing
/// but not yet past the window. Sets how fast a real deletion propagates and how
/// long a transient miss is tolerated.
const RESCAN_DELAY: Duration = Duration::from_secs(5);
const SCAN_DB_BATCH_SIZE: usize = 256;
const HASH_QUEUE_CAPACITY: usize = 256;
const HASH_RESULT_QUEUE_CAPACITY: usize = 256;
const THUMBNAIL_EVENT_BATCH_SIZE: usize = 50;
const THUMBNAIL_WORKER_LIMIT: usize = 4;
const THUMBNAIL_QUEUE_CAPACITY: usize = 128;

pub use crate::DownloadState;

#[derive(Debug, Clone, Serialize)]
pub enum LibraryEvent {
    BookAdded { content_hash: ContentHash, path: String },
    BookRemoved { content_hash: ContentHash, path: String },
    DirectoryAdded { path: String },
    DirectoryRemoved { path: String },
    ThumbnailGenerated { content_hash: ContentHash },
    ThumbnailBatchProcessed { content_hashes: Vec<ContentHash>, finished: bool },
    SubjectEnrichmentBatchProcessed { processed: usize, updated: usize, finished: bool },
    DownloadStatusChanged { content_hash: ContentHash, state: DownloadState },
    FolderDownloadStatusChanged { dir_id: crate::DirId, downloaded_books: usize, total_books: usize },
    TransferStatusChanged,
    CatalogueChanged,
    ScanComplete,
}

pub struct LibraryScanner {
    state: Arc<ScannerState>,
    watcher_thread: Option<thread::JoinHandle<()>>,
    worker_thread: Option<thread::JoinHandle<()>>,
    thumbnail_thread: Option<thread::JoinHandle<()>>,
    hash_inserter_thread: Option<thread::JoinHandle<()>>,
    hash_threads: Vec<thread::JoinHandle<()>>,
}

struct ScannerState {
    context: BackendContext,
    library_root: PathBuf,
    db: Pool<SqliteConnectionManager>,
    park_thread: OnceLock<thread::Thread>,
    scan_enabled: AtomicBool,
    scan_requested: AtomicBool,
    scan_active: AtomicBool,
    should_finish: AtomicBool,
    thumbnail_queue: crossbeam_channel::Sender<ThumbnailWork>,
    hash_queue: crossbeam_channel::Sender<HashTask>,
    hash_result_queue: crossbeam_channel::Sender<HashResult>,
    hash_result_rx: crossbeam_channel::Receiver<HashResult>,
    pending_hash_jobs: std::sync::Mutex<HashSet<ScanFileKey>>,
    pending_thumbnail_jobs: Arc<std::sync::Mutex<HashSet<ContentHash>>>,
    hash_jobs_changed: Condvar,
    hash_persist_failed: AtomicBool,
    event_tx: Sender<LibraryEvent>,
    #[cfg(test)]
    thumbnail_tasks_started: std::sync::atomic::AtomicUsize,
}

/// Clears the public scanning state before announcing completion, including
/// early-return error paths. Consumers can therefore handle `ScanComplete`
/// and immediately observe a settled scanner instead of racing the worker.
struct ScanActivity<'a>(&'a ScannerState);

impl Drop for ScanActivity<'_> {
    fn drop(&mut self) {
        self.0.scan_active.store(false, Ordering::Release);
        let _ = self.0.event_tx.send(LibraryEvent::ScanComplete);
        let _ = self.0.thumbnail_queue.send(ThumbnailWork::ScanFinished);
    }
}

struct ThumbnailTask {
    content_hash: ContentHash,
    book_path: PathBuf,
}

enum ThumbnailWork {
    Generate(ThumbnailTask),
    ScanFinished,
}

#[derive(Default)]
struct ThumbnailEventBatch {
    processed: usize,
    content_hashes: Vec<ContentHash>,
}

impl ThumbnailEventBatch {
    fn processed(&mut self, content_hash: Option<ContentHash>) -> Option<LibraryEvent> {
        self.content_hashes.extend(content_hash);
        self.processed += 1;
        (self.processed == THUMBNAIL_EVENT_BATCH_SIZE).then(|| self.take_event(false))
    }

    fn finished(&mut self) -> LibraryEvent {
        self.take_event(true)
    }

    fn take_event(&mut self, finished: bool) -> LibraryEvent {
        self.processed = 0;
        LibraryEvent::ThumbnailBatchProcessed { content_hashes: std::mem::take(&mut self.content_hashes), finished }
    }
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
    scan_id: i64,
    metadata_backfill: bool,
}

type BookMetadata = library_model::BookMetadataState;

enum HashResult {
    Ready { task: HashTask, content_hash: ContentHash, format: BookFormat, metadata: Option<BookMetadata>, audiobook: Option<book_metadata::AudiobookMetadata> },
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
        let library_root = Path::new(&library_path).canonicalize()?;
        if !library_root.is_dir() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("library root is not a directory: {library_path}")));
        }
        Self::from_root(context, library_root, db, event_tx)
    }

    pub(crate) fn from_root(context: BackendContext, library_root: PathBuf, db: Pool<SqliteConnectionManager>, event_tx: Sender<LibraryEvent>) -> std::io::Result<Self> {
        let (thumbnail_tx, thumbnail_rx) = crossbeam_channel::unbounded::<ThumbnailWork>();
        let (hash_tx, hash_rx) = crossbeam_channel::bounded::<HashTask>(HASH_QUEUE_CAPACITY);
        let (hash_result_tx, hash_result_rx) = crossbeam_channel::bounded::<HashResult>(HASH_RESULT_QUEUE_CAPACITY);

        let state = Arc::new(ScannerState {
            context,
            library_root,
            db,
            park_thread: OnceLock::new(),
            scan_enabled: AtomicBool::new(false),
            scan_requested: AtomicBool::new(false),
            scan_active: AtomicBool::new(false),
            should_finish: AtomicBool::new(false),
            thumbnail_queue: thumbnail_tx,
            hash_queue: hash_tx,
            hash_result_queue: hash_result_tx,
            hash_result_rx,
            pending_hash_jobs: std::sync::Mutex::new(HashSet::new()),
            pending_thumbnail_jobs: Arc::new(std::sync::Mutex::new(HashSet::new())),
            hash_jobs_changed: Condvar::new(),
            hash_persist_failed: AtomicBool::new(false),
            event_tx: event_tx.clone(),
            #[cfg(test)]
            thumbnail_tasks_started: std::sync::atomic::AtomicUsize::new(0),
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

        let inserter_state = Arc::clone(&state);
        let hash_inserter_thread = thread::spawn(move || Self::hash_inserter_loop(inserter_state));

        let thumbnail_state = Arc::clone(&state);
        let thumbnail_thread = thread::spawn(move || Self::thumbnail_worker_loop(thumbnail_state, thumbnail_rx));

        Ok(Self { state, watcher_thread: Some(watcher_thread), worker_thread: Some(worker_thread), thumbnail_thread: Some(thumbnail_thread), hash_inserter_thread: Some(hash_inserter_thread), hash_threads })
    }

    pub(crate) fn pending_thumbnail_jobs(&self) -> Arc<std::sync::Mutex<HashSet<ContentHash>>> {
        Arc::clone(&self.state.pending_thumbnail_jobs)
    }

    pub(crate) fn is_scanning(&self) -> bool {
        self.state.scan_active.load(Ordering::Acquire) || (self.state.scan_enabled.load(Ordering::Acquire) && self.state.scan_requested.load(Ordering::Acquire))
    }

    pub(crate) fn start_scanning(&self) {
        self.state.scan_enabled.store(true, Ordering::Release);
        Self::request_scan(&self.state);
    }

    fn scan_library(db: &Pool<SqliteConnectionManager>, state: &Arc<ScannerState>) {
        state.scan_active.store(true, Ordering::Release);
        let _activity = ScanActivity(state);
        let mut conn = match db.get() {
            Ok(c) => c,
            Err(e) => {
                log::error!("Scanner: Failed to get connection from pool: {}", e);
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
        let known_by_fingerprint = match get_book_dirs_by_local_fingerprint(&conn) {
            Ok(known) => known,
            Err(error) => {
                log::error!("Scanner: failed to preload local fingerprints: {error}");
                return;
            }
        };
        let tombstoned_book_files = match get_tombstoned_book_file_keys(&conn) {
            Ok(tombstones) => tombstones,
            Err(error) => {
                log::error!("Scanner: failed to preload tombstoned files: {error}");
                return;
            }
        };
        let mut known_directories = match db::get_known_directories_for_scan(&conn) {
            Ok(directories) => directories,
            Err(error) => {
                log::error!("Scanner: failed to preload directories: {error}");
                return;
            }
        };
        // Use IMMEDIATE transaction to acquire write lock immediately
        // This prevents race conditions where multiple transactions try to upgrade to exclusive
        let mut transaction = match conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate) {
            Ok(t) => t,
            Err(e) => {
                log::error!("Scanner: Failed to start transaction: {}", e);
                return;
            }
        };
        let library_root = state.library_root.as_path().to_path_buf();
        let mut walkdir = WalkDir::new(state.library_root.as_path()).follow_links(false).min_depth(1).into_iter();
        let root_dir_id = sync_common::ROOT_DIR_ID;
        let mut uuid_stack = vec![root_dir_id.clone()];
        let mut seen_dir_ids = HashSet::from([root_dir_id]);
        let mut suppressed_dir_ids = HashSet::new();
        let mut batch_entries = 0_usize;
        let mut pending_hash_tasks = Vec::new();
        let mut pending_seen_placements = Vec::new();
        state.hash_persist_failed.store(false, Ordering::Release);
        while let Some(next_entry) = walkdir.next() {
            if batch_entries >= SCAN_DB_BATCH_SIZE || pending_hash_tasks.len() >= HASH_QUEUE_CAPACITY {
                if let Err(error) = mark_known_book_dirs_seen(&transaction, scan_id, &pending_seen_placements) {
                    log::error!("Scanner: failed to mark observed files: {error}");
                    return;
                }
                pending_seen_placements.clear();
                if let Err(error) = transaction.commit() {
                    log::error!("Scanner: failed to commit scan batch: {error}");
                    return;
                }
                if !Self::queue_hash_tasks(state, std::mem::take(&mut pending_hash_tasks)) || !Self::wait_for_hash_jobs(state) {
                    return;
                }
                transaction = match conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate) {
                    Ok(transaction) => transaction,
                    Err(error) => {
                        log::error!("Scanner: failed to start the next scan batch: {error}");
                        return;
                    }
                };
                batch_entries = 0;
            }
            let entry = match next_entry {
                Ok(entry) => entry,
                Err(error) => {
                    log::warn!("Scanner: skipping unreadable entry: {error}");
                    continue;
                }
            };
            batch_entries += 1;
            if entry.file_name().to_string_lossy().starts_with('.') {
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
                let directory_name = raw_name.to_owned();
                let uuid_path = entry.path().join(".biblos_uuid");
                let mut dir_id = match fs::read_to_string(&uuid_path) {
                    Ok(uuid) => match DirId::parse_str(uuid.trim()) {
                        Ok(id) => id,
                        Err(error) => {
                            log::warn!("Scanner: invalid directory marker {}: {error}", uuid_path.display());
                            walkdir.skip_current_dir();
                            continue;
                        }
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        let dir_id = DirId::new_v4();
                        if let Err(error) = fs::write(&uuid_path, dir_id.to_string()) {
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
                let parent_is_suppressed = suppressed_dir_ids.contains(&parent_dir_id);
                match known_directories.get(&dir_id).map(|directory| &directory.state) {
                    Some(db::KnownDirectoryState::Deleted) => {
                        seen_dir_ids.insert(dir_id.clone());
                        suppressed_dir_ids.insert(dir_id.clone());
                        uuid_stack.push(dir_id);
                        continue;
                    }
                    Some(db::KnownDirectoryState::Live(Some(expected_path))) if expected_path.as_path() != relative => {
                        let expected = expected_path.resolve_under(&state.library_root);
                        if !expected.exists() && db::materialize_dir_parent_with_markers(&transaction, state.library_root.as_path(), &dir_id.to_string()).is_ok() && fs::rename(entry.path(), &expected).is_ok() {
                            drop(transaction);
                            Self::request_scan(state);
                            return;
                        }
                        log::warn!("Scanner: skipping stale path {} for directory {} (expected {})", entry.path().display(), dir_id, expected.display());
                        walkdir.skip_current_dir();
                        continue;
                    }
                    None if parent_is_suppressed => {
                        seen_dir_ids.insert(dir_id.clone());
                        suppressed_dir_ids.insert(dir_id.clone());
                        uuid_stack.push(dir_id);
                        continue;
                    }
                    Some(db::KnownDirectoryState::Live(_)) | None => {}
                }
                if !seen_dir_ids.insert(dir_id.clone()) {
                    dir_id = DirId::new_v4();
                    if let Err(error) = fs::write(&uuid_path, dir_id.to_string()) {
                        log::error!("Scanner: cannot replace duplicate directory marker {}: {error}", uuid_path.display());
                        walkdir.skip_current_dir();
                        continue;
                    }
                    seen_dir_ids.insert(dir_id.clone());
                }
                let directory_changed = match known_directories.get(&dir_id) {
                    Some(known) => known.parent_id != parent_dir_id || known.name != directory_name,
                    None => true,
                };
                if directory_changed {
                    if let Err(error) = upsert_dir(&transaction, &dir_id, &parent_dir_id, &directory_name) {
                        log::error!("Scanner: failed to persist directory {}: {error}", entry.path().display());
                        return;
                    }
                    let Some(relative) = relative.to_str() else {
                        log::error!("Scanner: persisted directory has a non-UTF-8 relative path: {}", entry.path().display());
                        return;
                    };
                    let relative_path = match crate::RelativeDirPath::parse(relative) {
                        Ok(path) => path,
                        Err(error) => {
                            log::error!("Scanner: persisted directory has an invalid relative path {}: {error}", entry.path().display());
                            return;
                        }
                    };
                    known_directories.insert(dir_id.clone(), db::KnownDirectory { parent_id: parent_dir_id.clone(), name: directory_name.clone(), state: db::KnownDirectoryState::Live(Some(relative_path)) });
                }
                uuid_stack.push(dir_id);
            } else if entry.file_type().is_file() && Self::is_supported_book_file(entry.path()) {
                let Some(raw_file_name) = entry.file_name().to_str() else {
                    log::warn!("Scanner: skipping EPUB with a non-UTF-8 name: {}", entry.path().display());
                    continue;
                };
                let file_name = raw_file_name.to_owned();
                let relative_path = match relative.to_str().and_then(|path| crate::RelativeBookPath::parse(path).ok()) {
                    Some(relative_path) => relative_path,
                    None => {
                        log::warn!("Scanner: skipping EPUB with an invalid relative path: {}", entry.path().display());
                        continue;
                    }
                };
                if tombstoned_book_files.contains(&(parent_dir_id, file_name.clone())) {
                    continue;
                }
                if suppressed_dir_ids.contains(&parent_dir_id) {
                    continue;
                }
                let file_hash = match db::local_file_fingerprint(entry.path()) {
                    Ok(file_hash) => file_hash,
                    Err(error) => {
                        log::warn!("Scanner: cannot fingerprint {}: {error}", entry.path().display());
                        continue;
                    }
                };
                let book_dir = known_by_fingerprint.get(&file_hash).cloned();
                let metadata_backfill = match book_dir {
                    // A known fast hash identifies a file already indexed by this
                    // library. It needs neither content hashing nor another
                    // placement; the existing scan entry remains authoritative.
                    Some(known) if !known.description_missing => {
                        // Skipping content inspection must not make the existing
                        // row look stale to this scan's cleanup pass. Accumulate
                        // these observations so one UPDATE marks the whole batch.
                        pending_seen_placements.push((known.dir_id, known.file_name, file_hash));
                        match thumbnail_needs_repair(&state.context, &state.library_root, &known.content_hash) {
                            Ok(true) => Self::queue_thumbnail_repair(state, ThumbnailTask { content_hash: known.content_hash, book_path: entry.path().to_path_buf() }),
                            Ok(false) => {}
                            Err(error) => log::warn!("Scanner: cannot inspect thumbnail state for {}: {error}", entry.path().display()),
                        }
                        continue;
                    }
                    Some(_) => true,
                    None => false,
                };
                // Unknown files need a portable content hash. Known files with
                // incomplete metadata currently share this worker path, but are
                // marked explicitly so their metadata may be backfilled without
                // overwriting edited metadata for an unrelated mtime-only change.
                let key = ScanFileKey { dir_id: parent_dir_id, file_name, rel_path: relative_path, fingerprint: file_hash };
                let task = HashTask { key: key.clone(), book_path: entry.path().to_path_buf(), scan_id, metadata_backfill };
                pending_hash_tasks.push(task);
            }
        }
        if let Err(error) = mark_known_book_dirs_seen(&transaction, scan_id, &pending_seen_placements) {
            log::error!("Scanner: failed to mark final observed files: {error}");
            return;
        }
        match transaction.commit() {
            Ok(_) => (),
            Err(e) => {
                log::error!("Scanner: Failed to commit transaction: {}", e);
                return;
            }
        }
        if !Self::queue_hash_tasks(state, pending_hash_tasks) || !Self::wait_for_hash_jobs(state) {
            return;
        }

        // Missing files change only this device's availability state. They do
        // not create deletion intent or emit synchronized removals.
        if let Ok(conn) = state.db.get() {
            if let Err(error) = db::mark_vanished_book_dirs_unavailable(&conn, scan_id) {
                log::error!("Scanner: failed to reconcile vanished placements: {error}");
                return;
            }
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
    }

    fn queue_hash_tasks(state: &Arc<ScannerState>, tasks: Vec<HashTask>) -> bool {
        for task in tasks {
            if state.should_finish.load(Ordering::Acquire) {
                return false;
            }
            let key = task.key.clone();
            {
                let mut pending = state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if !pending.insert(key.clone()) {
                    continue;
                }
            }
            if state.hash_queue.send(task).is_err() {
                state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&key);
                state.hash_jobs_changed.notify_all();
                return false;
            }
        }
        true
    }

    fn queue_thumbnail_repair(state: &Arc<ScannerState>, task: ThumbnailTask) {
        let content_hash = task.content_hash.clone();
        let inserted = state.pending_thumbnail_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(content_hash.clone());
        if inserted && state.thumbnail_queue.send(ThumbnailWork::Generate(task)).is_err() {
            state.pending_thumbnail_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&content_hash);
        }
    }

    fn wait_for_hash_jobs(state: &Arc<ScannerState>) -> bool {
        let mut pending = state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        while !pending.is_empty() && !state.should_finish.load(Ordering::Acquire) {
            pending = state.hash_jobs_changed.wait_timeout(pending, Duration::from_millis(250)).unwrap_or_else(std::sync::PoisonError::into_inner).0;
        }
        !state.should_finish.load(Ordering::Acquire) && !state.hash_persist_failed.load(Ordering::Acquire)
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
        self.state.scan_enabled.store(true, Ordering::Release);
        self.state.scan_requested.store(true, Ordering::Release);
        self.state.hash_jobs_changed.notify_all();
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
        self.hash_inserter_thread.take();
        self.thumbnail_thread.take();
    }
}

impl LibraryScanner {
    fn hash_worker_loop(state: Arc<ScannerState>, hash_rx: crossbeam_channel::Receiver<HashTask>) {
        let mut hash_buffer = vec![0_u8; 1024 * 1024];
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
                // A local library is a catalogue of the user's files, not an
                // upload trust boundary. Full reader/upload validation rejects
                // many otherwise readable legacy publications (for example
                // EPUB page-template resources and PDFs containing URI actions),
                // which made those files disappear from the library entirely.
                // Keep strict validation at import/upload/open boundaries and
                // let the scanner index every supported local file extension.
                let format = Self::book_format_from_path(&task.book_path).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "unsupported book extension"))?;
                let content_hash = generate_content_hash_blake3(&task.book_path, &mut hash_buffer)?;
                let (metadata, audiobook) = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| book_metadata::inspect_book(&task.book_path, format))) {
                    Ok(Ok(inspection)) => (Some(inspection.metadata), inspection.audiobook),
                    Ok(Err(error)) => {
                        log::warn!("Skipping metadata read for {}: {error}", task.book_path.display());
                        (None, None)
                    }
                    Err(_) => {
                        log::warn!("Skipping metadata parser panic for {}", task.book_path.display());
                        (None, None)
                    }
                };
                Ok::<_, std::io::Error>((content_hash, format, metadata, audiobook))
            })) {
                Ok(Ok((content_hash, format, metadata, audiobook))) => HashResult::Ready { task, content_hash, format, metadata, audiobook },
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

    fn hash_inserter_loop(state: Arc<ScannerState>) {
        const MAX_BATCH: usize = 64;
        loop {
            let first = match state.hash_result_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(item) => item,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) if !state.should_finish.load(Ordering::Acquire) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Timeout | crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            };
            let mut batch = vec![first];
            while batch.len() < MAX_BATCH {
                match state.hash_result_rx.try_recv() {
                    Ok(item) => batch.push(item),
                    Err(crossbeam_channel::TryRecvError::Empty | crossbeam_channel::TryRecvError::Disconnected) => break,
                }
            }
            Self::persist_hash_batch(&state, batch);
        }
        let mut pending = state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !pending.is_empty() {
            state.hash_persist_failed.store(true, Ordering::Release);
            pending.clear();
        }
        state.hash_jobs_changed.notify_all();
    }

    fn persist_hash_batch(state: &Arc<ScannerState>, batch: Vec<HashResult>) {
        for item in &batch {
            if let HashResult::Failed { task, error } = item {
                log::warn!("Scanner: failed to hash {}: {error}", task.book_path.display());
            }
        }

        let mut thumbnails = Vec::new();
        let ready_exists = batch.iter().any(|item| matches!(item, HashResult::Ready { .. }));
        let persisted = if !ready_exists { true } else { Self::persist_ready_hashes(state, &batch, &mut thumbnails) };
        if !persisted {
            state.hash_persist_failed.store(true, Ordering::Release);
        } else {
            for task in thumbnails {
                Self::queue_thumbnail_repair(state, task);
            }
        }

        let mut pending = state.pending_hash_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for item in &batch {
            pending.remove(item.key());
        }
        drop(pending);
        state.hash_jobs_changed.notify_all();
    }

    fn persist_ready_hashes(state: &Arc<ScannerState>, batch: &[HashResult], thumbnails: &mut Vec<ThumbnailTask>) -> bool {
        let mut conn = match state.db.get() {
            Ok(conn) => conn,
            Err(error) => {
                log::error!("Scanner inserter: failed to get DB connection: {error}");
                return false;
            }
        };
        let tx = match conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate) {
            Ok(tx) => tx,
            Err(error) => {
                log::error!("Scanner inserter: failed to start transaction: {error}");
                return false;
            }
        };
        for item in batch {
            let HashResult::Ready { task, content_hash, format, metadata, audiobook } = item else { continue };
            match db::is_book_file_tombstoned(&tx, &task.key.dir_id, &task.key.file_name) {
                Ok(true) => continue,
                Ok(false) => match db::get_known_directory_scan_state(&tx, &task.key.dir_id) {
                    Ok(Some(db::KnownDirectoryState::Deleted)) => continue,
                    Ok(_) => {}
                    Err(error) => {
                        log::error!("Scanner inserter: failed to check directory state: {error}");
                        return false;
                    }
                },
                Err(error) => {
                    log::error!("Scanner inserter: failed to check tombstone state: {error}");
                    return false;
                }
            }
            let already_indexed = match db::book_has_live_placement(&tx, &task.key.dir_id, content_hash.clone()) {
                Ok(value) => value,
                Err(error) => {
                    log::error!("Scanner inserter: failed to check duplicate placement: {error}");
                    return false;
                }
            };
            let metadata_result = match (already_indexed, task.metadata_backfill, metadata) {
                (true, false, _) => Ok(()),
                (_, _, Some(metadata)) => db::insert_scanned_book_metadata(&tx, content_hash.clone(), metadata, *format),
                (_, _, None) => insert_book(&tx, content_hash.clone(), *format),
            };
            if let Err(error) = metadata_result.and_then(|_| insert_local_book_dir(&tx, &task.key.dir_id, content_hash.clone(), &task.key.file_name, task.key.fingerprint, task.scan_id)) {
                log::error!("Scanner inserter: database batch failed: {error}");
                return false;
            }
            if !already_indexed || task.metadata_backfill {
                if let Some(audiobook) = audiobook {
                    if let Err(error) = db::upsert_audiobook_metadata(&tx, content_hash, audiobook) {
                        log::error!("Scanner inserter: failed to persist audiobook metadata: {error}");
                        return false;
                    }
                }
            }
            if !already_indexed {
                thumbnails.push(ThumbnailTask { content_hash: content_hash.clone(), book_path: task.book_path.clone() });
            }
        }
        if let Err(error) = tx.commit() {
            log::error!("Scanner inserter: failed to commit transaction: {error}");
            return false;
        }
        true
    }

    fn worker_loop(state: Arc<ScannerState>) {
        let current_thread = thread::current();
        let _ = state.park_thread.set(current_thread);
        let mut last_scan_time: Option<Instant> = None;

        loop {
            if state.should_finish.load(Ordering::Acquire) {
                break;
            }
            if !state.scan_enabled.load(Ordering::Acquire) {
                thread::park();
                continue;
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

    fn thumbnail_worker_loop(state: Arc<ScannerState>, thumbnail_rx: crossbeam_channel::Receiver<ThumbnailWork>) {
        const FLUSH_INTERVAL: Duration = Duration::from_millis(1000);
        let worker_count = thread::available_parallelism().map(|count| count.get()).unwrap_or(2).clamp(1, THUMBNAIL_WORKER_LIMIT);
        let image_worker_count = worker_count.saturating_sub(1).max(1);
        let (pdf_task_tx, pdf_task_rx) = crossbeam_channel::bounded::<ThumbnailTask>(THUMBNAIL_QUEUE_CAPACITY);
        let (image_task_tx, image_task_rx) = crossbeam_channel::bounded::<ThumbnailTask>(THUMBNAIL_QUEUE_CAPACITY);
        let (result_tx, result_rx) = crossbeam_channel::unbounded::<Option<ContentHash>>();

        thread::scope(|scope| {
            {
                let worker_state = Arc::clone(&state);
                let result_tx = result_tx.clone();
                scope.spawn(move || Self::thumbnail_task_loop(worker_state, pdf_task_rx, result_tx));
            }
            for _ in 0..image_worker_count {
                let worker_state = Arc::clone(&state);
                let task_rx = image_task_rx.clone();
                let result_tx = result_tx.clone();
                scope.spawn(move || Self::thumbnail_task_loop(worker_state, task_rx, result_tx));
            }
            drop(result_tx);

            let mut batch = ThumbnailEventBatch::default();
            let mut outstanding = 0_usize;
            let mut queued = Vec::new();
            'coordinator: loop {
                match thumbnail_rx.recv_timeout(FLUSH_INTERVAL) {
                    Ok(ThumbnailWork::Generate(task)) => {
                        // Indexing has priority. Starting image codecs while hash
                        // and metadata workers are still active delayed
                        // ScanComplete and competed with the interactive UI.
                        queued.push(task);
                    }
                    Ok(ThumbnailWork::ScanFinished) => {
                        for task in queued.drain(..) {
                            let sent = if Self::book_format_from_path(&task.book_path) == Some(BookFormat::Pdf) { pdf_task_tx.send(task) } else { image_task_tx.send(task) };
                            if sent.is_err() {
                                break 'coordinator;
                            }
                            outstanding += 1;
                            Self::drain_thumbnail_results(&state, &result_rx, &mut batch, &mut outstanding);
                        }
                        while outstanding > 0 {
                            if state.should_finish.load(Ordering::Acquire) {
                                break 'coordinator;
                            }
                            match result_rx.recv_timeout(FLUSH_INTERVAL) {
                                Ok(content_hash) => Self::record_thumbnail_result(&state, &mut batch, content_hash, &mut outstanding),
                                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break 'coordinator,
                            }
                        }
                        let _ = state.event_tx.send(batch.finished());
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        Self::drain_thumbnail_results(&state, &result_rx, &mut batch, &mut outstanding);
                        if state.should_finish.load(Ordering::Acquire) {
                            break;
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                }
            }
            drop(pdf_task_tx);
            drop(image_task_tx);
        });
    }

    fn thumbnail_task_loop(state: Arc<ScannerState>, task_rx: crossbeam_channel::Receiver<ThumbnailTask>, result_tx: crossbeam_channel::Sender<Option<ContentHash>>) {
        const POLL_INTERVAL: Duration = Duration::from_millis(1000);
        while !state.should_finish.load(Ordering::Acquire) {
            let task = match task_rx.recv_timeout(POLL_INTERVAL) {
                Ok(task) => task,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            };
            #[cfg(test)]
            state.thumbnail_tasks_started.fetch_add(1, Ordering::Relaxed);
            let content_hash = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| generate_thumbnail_for_sync(&state.context, &state.library_root, &task.book_path, &task.content_hash)))
                .ok()
                .and_then(Result::ok)
                .filter(|available| *available)
                .map(|_| task.content_hash.clone());
            state.pending_thumbnail_jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&task.content_hash);
            if result_tx.send(content_hash).is_err() {
                break;
            }
        }
    }

    fn drain_thumbnail_results(state: &ScannerState, result_rx: &crossbeam_channel::Receiver<Option<ContentHash>>, batch: &mut ThumbnailEventBatch, outstanding: &mut usize) {
        while let Ok(content_hash) = result_rx.try_recv() {
            Self::record_thumbnail_result(state, batch, content_hash, outstanding);
        }
    }

    fn record_thumbnail_result(state: &ScannerState, batch: &mut ThumbnailEventBatch, content_hash: Option<ContentHash>, outstanding: &mut usize) {
        *outstanding = outstanding.saturating_sub(1);
        if let Some(event) = batch.processed(content_hash) {
            let _ = state.event_tx.send(event);
        }
    }

    #[cfg(feature = "watcher")]
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

    #[cfg(not(feature = "watcher"))]
    fn watcher_loop(_state: Arc<ScannerState>) {}

    #[cfg(feature = "watcher")]
    fn should_trigger_scan(event: &Event) -> bool {
        if event.paths.is_empty() {
            return true;
        }
        event.paths.iter().any(|path| Self::is_relevant_path(path))
    }

    fn request_scan(state: &ScannerState) {
        state.scan_requested.store(true, Ordering::Release);
        if state.scan_enabled.load(Ordering::Acquire) {
            if let Some(thread) = state.park_thread.get() {
                thread.unpark();
            }
        }
    }

    #[cfg(feature = "watcher")]
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

    fn book_format_from_path(path: &Path) -> Option<BookFormat> {
        let extension = path.extension()?.to_str()?;
        match extension.to_ascii_lowercase().as_str() {
            extension => BookFormat::from_extension(extension),
        }
    }
}

use crossbeam_channel::Sender;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
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
    use tempfile::tempdir;
    use test_support::{fixture_content_hash, fixture_dir_id};

    fn test_scanner_state(context: &BackendContext, library_root: &Path) -> (Arc<ScannerState>, crossbeam_channel::Receiver<HashTask>, crossbeam_channel::Receiver<LibraryEvent>) {
        let db_path = library_root.join(APP_HIDDEN_LIBRARY_DB_PATH);
        fs::create_dir_all(db_path.parent().expect("library db parent exists")).unwrap();

        let manager = SqliteConnectionManager::file(db_path).with_init(|conn| db::configure_connection(conn));
        let db = Pool::builder().max_size(4).connection_timeout(Duration::from_secs(30)).build(manager).unwrap();
        {
            let conn = db.get().unwrap();
            db::init_db(&conn).unwrap();
        }

        let (thumbnail_tx, _thumbnail_rx) = crossbeam_channel::unbounded::<ThumbnailWork>();
        let (hash_tx, hash_rx) = crossbeam_channel::bounded::<HashTask>(HASH_QUEUE_CAPACITY);
        let (hash_result_tx, hash_result_rx) = crossbeam_channel::bounded::<HashResult>(HASH_RESULT_QUEUE_CAPACITY);
        let (event_tx, event_rx) = crossbeam_channel::unbounded::<LibraryEvent>();

        let state = Arc::new(ScannerState {
            context: context.clone(),
            library_root: library_root.to_path_buf(),
            db,
            park_thread: OnceLock::new(),
            scan_enabled: AtomicBool::new(true),
            scan_requested: AtomicBool::new(true),
            scan_active: AtomicBool::new(true),
            should_finish: AtomicBool::new(false),
            thumbnail_queue: thumbnail_tx,
            hash_queue: hash_tx,
            hash_result_queue: hash_result_tx,
            hash_result_rx,
            pending_hash_jobs: std::sync::Mutex::new(HashSet::new()),
            pending_thumbnail_jobs: Arc::new(std::sync::Mutex::new(HashSet::new())),
            hash_jobs_changed: Condvar::new(),
            hash_persist_failed: AtomicBool::new(false),
            event_tx,
            thumbnail_tasks_started: std::sync::atomic::AtomicUsize::new(0),
        });

        (state, hash_rx, event_rx)
    }

    #[test]
    fn pending_hash_identity_includes_placement_not_only_fingerprint() {
        let first = ScanFileKey { dir_id: fixture_dir_id("first"), file_name: "book.epub".to_owned(), rel_path: RelativeBookPath::parse("First/book.epub").unwrap(), fingerprint: 7 };
        let second = ScanFileKey { dir_id: fixture_dir_id("second"), file_name: "book.epub".to_owned(), rel_path: RelativeBookPath::parse("Second/book.epub").unwrap(), fingerprint: 7 };
        let mut pending = HashSet::new();
        assert!(pending.insert(first));
        assert!(pending.insert(second));
    }

    #[test]
    fn thumbnail_events_are_batched_and_finish_explicitly() {
        let mut batch = ThumbnailEventBatch::default();
        for index in 0..THUMBNAIL_EVENT_BATCH_SIZE - 1 {
            assert!(batch.processed(Some(fixture_content_hash(index as u64))).is_none());
        }
        let event = batch.processed(None).expect("the fiftieth processed thumbnail flushes the batch");
        match event {
            LibraryEvent::ThumbnailBatchProcessed { content_hashes, finished } => {
                assert_eq!(content_hashes.len(), THUMBNAIL_EVENT_BATCH_SIZE - 1);
                assert!(!finished);
            }
            _ => panic!("unexpected thumbnail event"),
        }
        match batch.finished() {
            LibraryEvent::ThumbnailBatchProcessed { content_hashes, finished } => {
                assert!(content_hashes.is_empty());
                assert!(finished);
            }
            _ => panic!("unexpected thumbnail event"),
        }
    }

    #[test]
    fn parallel_thumbnail_worker_finishes_after_all_queued_results() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let library_root = library_root_path.canonicalize().unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().join("app"))).unwrap();
        let (state, _hash_rx, event_rx) = test_scanner_state(&context, &library_root);
        let (thumbnail_tx, thumbnail_rx) = crossbeam_channel::unbounded();
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::thumbnail_worker_loop(worker_state, thumbnail_rx));

        for index in 0..75 {
            let content_hash = fixture_content_hash(index);
            state.pending_thumbnail_jobs.lock().unwrap().insert(content_hash.clone());
            thumbnail_tx.send(ThumbnailWork::Generate(ThumbnailTask { content_hash, book_path: library_root.join(format!("book-{index}.html")) })).unwrap();
        }
        thumbnail_tx.send(ThumbnailWork::ScanFinished).unwrap();

        match event_rx.recv_timeout(Duration::from_secs(10)).unwrap() {
            LibraryEvent::ThumbnailBatchProcessed { content_hashes, finished } => {
                assert!(content_hashes.is_empty());
                assert!(!finished);
            }
            event => panic!("unexpected thumbnail event: {event:?}"),
        }
        match event_rx.recv_timeout(Duration::from_secs(10)).unwrap() {
            LibraryEvent::ThumbnailBatchProcessed { content_hashes, finished } => {
                assert!(content_hashes.is_empty());
                assert!(finished);
            }
            event => panic!("unexpected thumbnail event: {event:?}"),
        }
        assert!(state.pending_thumbnail_jobs.lock().unwrap().is_empty());

        state.should_finish.store(true, Ordering::Release);
        drop(thumbnail_tx);
        worker.join().unwrap();
    }

    #[test]
    fn thumbnail_work_does_not_start_before_scan_finished() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let library_root = library_root_path.canonicalize().unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().join("app"))).unwrap();
        let (state, _hash_rx, event_rx) = test_scanner_state(&context, &library_root);
        let (thumbnail_tx, thumbnail_rx) = crossbeam_channel::unbounded();
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::thumbnail_worker_loop(worker_state, thumbnail_rx));

        thumbnail_tx.send(ThumbnailWork::Generate(ThumbnailTask { content_hash: fixture_content_hash(1), book_path: library_root.join("book.html") })).unwrap();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(state.thumbnail_tasks_started.load(Ordering::Relaxed), 0);

        thumbnail_tx.send(ThumbnailWork::ScanFinished).unwrap();
        assert!(matches!(event_rx.recv_timeout(Duration::from_secs(5)).unwrap(), LibraryEvent::ThumbnailBatchProcessed { finished: true, .. }));
        assert_eq!(state.thumbnail_tasks_started.load(Ordering::Relaxed), 1);

        state.should_finish.store(true, Ordering::Release);
        drop(thumbnail_tx);
        worker.join().unwrap();
    }

    #[test]
    fn scan_never_deletes_a_tombstoned_file() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let book_path = library_root_path.join("kept.epub");
        fs::write(&book_path, b"locally retained publication").unwrap();
        let library_root = library_root_path.canonicalize().unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().join("app"))).unwrap();
        let (state, _hash_rx, _event_rx) = test_scanner_state(&context, &library_root);
        let content_hash = fixture_content_hash(1);
        {
            let conn = state.db.get().unwrap();
            conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [content_hash.as_str()]).unwrap();
            conn.execute(
                "INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded, deleted_at) VALUES (?1, (SELECT row_id FROM book WHERE content_hash=?2), 'kept.epub', '', 0, 1)",
                rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), content_hash.as_str()],
            )
            .unwrap();
        }

        LibraryScanner::scan_library(&state.db, &state);

        assert!(book_path.exists());
        assert_eq!(fs::read(book_path).unwrap(), b"locally retained publication");
    }

    #[test]
    fn hash_worker_missing_file_emits_failure_and_clears_pending() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let library_root = library_root_path.canonicalize().unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().to_path_buf())).unwrap();
        let (state, hash_rx, _event_rx) = test_scanner_state(&context, &library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));
        let inserter_state = Arc::clone(&state);
        let inserter = thread::spawn(move || LibraryScanner::hash_inserter_loop(inserter_state));

        let key = ScanFileKey { dir_id: fixture_dir_id("dir-a"), file_name: "missing.epub".to_owned(), rel_path: RelativeBookPath::parse("missing.epub").unwrap(), fingerprint: 7 };
        let task = HashTask { key: key.clone(), book_path: library_root_path.join("missing.epub"), scan_id: 1, metadata_backfill: false };
        assert!(LibraryScanner::queue_hash_tasks(&state, vec![task]));
        assert!(LibraryScanner::wait_for_hash_jobs(&state));

        assert!(state.pending_hash_jobs.lock().unwrap().is_empty());

        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
        inserter.join().unwrap();
    }

    #[test]
    fn local_catalogue_keeps_supported_files_that_fail_strict_reader_validation() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        fs::write(library_root_path.join("legacy.epub"), b"legacy publication bytes").unwrap();
        let library_root = library_root_path.canonicalize().unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().join("app"))).unwrap();
        let (state, hash_rx, _event_rx) = test_scanner_state(&context, &library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));
        let inserter_state = Arc::clone(&state);
        let inserter = thread::spawn(move || LibraryScanner::hash_inserter_loop(inserter_state));

        LibraryScanner::scan_library(&state.db, &state);

        let conn = state.db.get().unwrap();
        let placements: i64 = conn.query_row("SELECT COUNT(*) FROM book_dir WHERE file_name = 'legacy.epub' AND deleted_at IS NULL", (), |row| row.get(0)).unwrap();
        assert_eq!(placements, 1);
        drop(conn);

        state.should_finish.store(true, Ordering::Release);
        state.hash_jobs_changed.notify_all();
        worker.join().unwrap();
        inserter.join().unwrap();
    }

    #[test]
    fn scan_backfills_metadata_for_an_existing_placement() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let book_path = library_root_path.join("legacy.pdf");
        fs::write(&book_path, b"local publication bytes").unwrap();
        let library_root = library_root_path.canonicalize().unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().join("app"))).unwrap();
        let (state, hash_rx, _event_rx) = test_scanner_state(&context, &library_root);
        let mut hash_buffer = Vec::new();
        let content_hash = generate_content_hash_blake3(&book_path, &mut hash_buffer).unwrap();
        let fingerprint = db::local_file_fingerprint(&book_path).unwrap();
        {
            let conn = state.db.get().unwrap();
            conn.execute("INSERT INTO book(content_hash, added_at, description_scanned, format) VALUES (?1, 42, 0, 'pdf')", [content_hash.as_str()]).unwrap();
            conn.execute(
                "INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, last_scan, is_downloaded) VALUES (?1, (SELECT row_id FROM book WHERE content_hash=?2), 'legacy.pdf', ?3, 1, 0)",
                rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), content_hash.as_str(), fingerprint.to_string()],
            )
            .unwrap();
        }
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));
        let inserter_state = Arc::clone(&state);
        let inserter = thread::spawn(move || LibraryScanner::hash_inserter_loop(inserter_state));

        LibraryScanner::scan_library(&state.db, &state);

        let conn = state.db.get().unwrap();
        let stored: (String, bool, i64) = conn.query_row("SELECT COALESCE(title, ''), description_scanned, added_at FROM book WHERE content_hash=?1", [content_hash.as_str()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        assert_eq!(stored, ("legacy".to_owned(), true, 42), "metadata backfill must preserve the original addition time");
        let downloaded: bool = conn.query_row("SELECT is_downloaded FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [content_hash.as_str()], |row| row.get(0)).unwrap();
        assert!(downloaded, "an existing file processed for metadata must still be marked present");
        drop(conn);

        state.should_finish.store(true, Ordering::Release);
        state.hash_jobs_changed.notify_all();
        worker.join().unwrap();
        inserter.join().unwrap();
    }

    /// Scanned files keep the name the user gave them, verbatim. Guards against
    /// anyone later "helpfully" normalizing scanned names to the download
    /// template: nothing about a filename is synchronized, and nothing on disk
    /// is ever renamed.
    /// Two folders carrying the same marker are not two identities. The first
    /// one walked keeps the id; the second is recognized as a stale path for a
    /// directory that already lives elsewhere, so it is skipped rather than
    /// silently re-identified into a duplicate directory row.
    #[test]
    fn a_folder_reusing_another_folders_marker_is_skipped_as_a_stale_path() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let library_root = library_root_path.canonicalize().unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().to_path_buf())).unwrap();

        let shared_uuid = "6f1a3c52-2f4e-4a1b-9f77-0c3d5e8b21aa";
        let first = library_root_path.join("First");
        let second = library_root_path.join("Second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        fs::write(first.join(".biblos_uuid"), shared_uuid).unwrap();
        fs::write(second.join(".biblos_uuid"), shared_uuid).unwrap();

        let (state, hash_rx, _event_rx) = test_scanner_state(&context, &library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));
        let inserter_state = Arc::clone(&state);
        let inserter = thread::spawn(move || LibraryScanner::hash_inserter_loop(inserter_state));

        LibraryScanner::scan_library(&state.db, &state);

        let conn = state.db.get().unwrap();
        let directories: i64 = conn.query_row("SELECT COUNT(*) FROM dir WHERE id != '00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL", (), |row| row.get(0)).unwrap();
        assert_eq!(directories, 1, "one marker is one directory identity");
        // Which folder wins depends on directory walk order, which is the
        // filesystem's business; what must hold is that exactly one of them owns
        // the identity and the other is left untouched on disk.
        let name: String = conn.query_row("SELECT name FROM dir WHERE id=?1", [shared_uuid], |row| row.get(0)).unwrap();
        assert!(name == "First" || name == "Second", "the surviving identity must be one of the two folders, got {name}");
        let surviving_name = name;
        drop(conn);

        // The loser is a stale path for a directory that lives elsewhere, and it
        // holds nothing, so it is cleaned up rather than re-identified. The
        // survivor keeps the marker it came with: no new identity is minted.
        let survivors: Vec<&Path> = [first.as_path(), second.as_path()].into_iter().filter(|path| path.exists()).collect();
        assert_eq!(survivors.len(), 1, "an empty duplicate folder is removed, not kept");
        assert_eq!(fs::read_to_string(survivors[0].join(".biblos_uuid")).unwrap(), shared_uuid);
        assert_eq!(survivors[0].file_name().unwrap().to_str().unwrap(), surviving_name, "the folder left on disk is the one that owns the identity");

        state.should_finish.store(true, Ordering::Release);
        state.hash_jobs_changed.notify_all();
        worker.join().unwrap();
        inserter.join().unwrap();
    }

    #[test]
    fn a_scanned_file_keeps_its_on_disk_name_and_renaming_it_emits_nothing() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        fs::write(library_root_path.join("my own name.epub"), b"scanned publication bytes").unwrap();
        let library_root = library_root_path.canonicalize().unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().join("app"))).unwrap();
        let (state, hash_rx, _event_rx) = test_scanner_state(&context, &library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));
        let inserter_state = Arc::clone(&state);
        let inserter = thread::spawn(move || LibraryScanner::hash_inserter_loop(inserter_state));

        LibraryScanner::scan_library(&state.db, &state);

        let conn = state.db.get().unwrap();
        let stored: String = conn.query_row("SELECT file_name FROM book_dir WHERE deleted_at IS NULL", (), |row| row.get(0)).unwrap();
        assert_eq!(stored, "my own name.epub", "the scanner records observed names verbatim");
        let kinds_after_scan: Vec<String> = conn.prepare("SELECT DISTINCT state_kind FROM sync_outbox ORDER BY state_kind").unwrap().query_map((), |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
        drop(conn);

        // The user renames the file. The register that used to synchronize names
        // is gone, so this must not enqueue anything at all.
        fs::rename(library_root_path.join("my own name.epub"), library_root_path.join("renamed.epub")).unwrap();
        LibraryScanner::scan_library(&state.db, &state);

        let conn = state.db.get().unwrap();
        let kinds_after_rename: Vec<String> = conn.prepare("SELECT DISTINCT state_kind FROM sync_outbox ORDER BY state_kind").unwrap().query_map((), |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
        assert_eq!(kinds_after_rename, kinds_after_scan, "a rename on disk must not introduce a new outbox state kind");
        assert!(!kinds_after_rename.iter().any(|kind| kind.contains("file_name")), "no filename register may exist: {kinds_after_rename:?}");
        drop(conn);

        state.should_finish.store(true, Ordering::Release);
        state.hash_jobs_changed.notify_all();
        worker.join().unwrap();
        inserter.join().unwrap();
    }

    #[test]
    fn large_scan_crosses_bounded_batches_and_repeat_scan_needs_no_hash_workers() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        fs::create_dir_all(library_root_path.join("Shelf/Topic/Subtopic")).unwrap();
        for index in 0..(HASH_QUEUE_CAPACITY + 44) {
            fs::write(library_root_path.join(format!("book-{index:03}.html")), format!("<!doctype html><html><head><title>Book {index}</title></head><body>{index}</body></html>")).unwrap();
        }
        let library_root = library_root_path.canonicalize().unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(app_data.path().join("app"))).unwrap();
        let (state, hash_rx, _event_rx) = test_scanner_state(&context, &library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));
        let inserter_state = Arc::clone(&state);
        let inserter = thread::spawn(move || LibraryScanner::hash_inserter_loop(inserter_state));

        LibraryScanner::scan_library(&state.db, &state);
        let conn = state.db.get().unwrap();
        let placements: i64 = conn.query_row("SELECT COUNT(*) FROM book_dir WHERE deleted_at IS NULL", (), |row| row.get(0)).unwrap();
        assert_eq!(placements, (HASH_QUEUE_CAPACITY + 44) as i64);
        let first_outbox: i64 = conn.query_row("SELECT COUNT(*) FROM sync_outbox", (), |row| row.get(0)).unwrap();
        conn.execute_batch(
            "CREATE TABLE dir_write_audit(operation TEXT NOT NULL);
             CREATE TRIGGER audit_dir_insert AFTER INSERT ON dir BEGIN
                 INSERT INTO dir_write_audit(operation) VALUES ('insert');
             END;
             CREATE TRIGGER audit_dir_update AFTER UPDATE ON dir BEGIN
                 INSERT INTO dir_write_audit(operation) VALUES ('update');
             END;",
        )
        .unwrap();
        drop(conn);

        state.should_finish.store(true, Ordering::Release);
        state.hash_jobs_changed.notify_all();
        worker.join().unwrap();
        inserter.join().unwrap();
        state.should_finish.store(false, Ordering::Release);

        LibraryScanner::scan_library(&state.db, &state);
        let conn = state.db.get().unwrap();
        let second_outbox: i64 = conn.query_row("SELECT COUNT(*) FROM sync_outbox", (), |row| row.get(0)).unwrap();
        assert_eq!(second_outbox, first_outbox, "an unchanged rescan must use the fingerprint preload and emit no new state");
        let directory_writes: i64 = conn.query_row("SELECT COUNT(*) FROM dir_write_audit", (), |row| row.get(0)).unwrap();
        assert_eq!(directory_writes, 0, "an unchanged rescan must not rewrite known directories");
        assert!(state.pending_hash_jobs.lock().unwrap().is_empty());
    }
}
