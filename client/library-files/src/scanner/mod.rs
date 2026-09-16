use super::asset_store::AssetStore;
use crate::database::FileDatabase;
use super::events::{ScanEvent as LibraryEvent, ScanEventSink as LibraryEventSender};
#[cfg(test)]
use crate::APP_HIDDEN_LIBRARY_DB_PATH;
use client_library::scanner::get_book_dirs_by_local_fingerprint;
use client_library::scanner::get_tombstoned_book_file_keys;
use client_library::scanner::insert_book;
use client_library::scanner::insert_local_book_dir;
use client_library::scanner::mark_known_book_dirs_seen;
use client_library::placements::upsert_dir;
#[cfg(feature = "watcher")]
use crate::APP_HIDDEN_DIR;
use crate::{ContentHash, DirId};
use library_model::BookFormat;
#[cfg(feature = "watcher")]
use notify::{Event, RecursiveMode, Watcher, recommended_watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "watcher")]
use std::sync::mpsc::channel as std_channel;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use std::time::Instant;
use std::{fs, thread};
use walkdir::WalkDir;

mod estimate;
pub use estimate::{estimate_folders, estimate_folders_cancellable};

mod pending;
mod result_batch;
use pending::{file_work_revision, PendingScanWork};

/// Delay before re-scanning to advance the vanish grace count for files missing
/// but not yet past the window. Only complete readable scans infer deletions.
const RESCAN_DELAY: Duration = Duration::from_secs(5);
const SCAN_DB_BATCH_SIZE: usize = 256;
const HASH_QUEUE_CAPACITY: usize = 256;
const HASH_RESULT_QUEUE_CAPACITY: usize = 256;

fn ensure_publication_identity(path: &Path) -> std::io::Result<ContentHash> {
    publication_identity::ensure(path)
}

fn local_file_fingerprint(path: &Path) -> std::io::Result<u64> {
    client_platform_native::file_fingerprint::file_metadata_fingerprint(&fs::metadata(path)?)
}

pub struct LibraryScanner<D: FileDatabase> {
    state: Arc<ScannerState<D>>,
    watcher_thread: Option<thread::JoinHandle<()>>,
    worker_thread: Option<thread::JoinHandle<()>>,
    hash_threads: Vec<thread::JoinHandle<()>>,
}

struct ScannerState<D: FileDatabase> {
    library_root: PathBuf,
    db: D,
    assets: AssetStore,
    park_thread: OnceLock<thread::Thread>,
    scan_enabled: AtomicBool,
    scan_requested: AtomicBool,
    scan_active: AtomicBool,
    should_finish: AtomicBool,
    hash_queue: crossbeam_channel::Sender<HashTask>,
    hash_result_queue: crossbeam_channel::Sender<HashResult>,
    hash_result_rx: crossbeam_channel::Receiver<HashResult>,
    event_tx: LibraryEventSender,
    progress: Arc<std::sync::Mutex<crate::ScanProgress>>,
}

/// Clears the public scanning state before announcing completion, including
/// early-return error paths. Consumers can therefore handle `ScanComplete`
/// and immediately observe a settled scanner instead of racing the worker.
struct ScanActivity<'a, D: FileDatabase> {
    state: &'a ScannerState<D>,
    successful: bool,
}

impl<D: FileDatabase> Drop for ScanActivity<'_, D> {
    fn drop(&mut self) {
        let progress = *self.state.progress.lock().unwrap();
        let result = self.state.db.get().and_then(|conn| {
            client_library::operations::finish_scan(&conn, progress.total, progress.failed, self.successful).map_err(Into::into)
        });
        if let Err(error) = result { log::warn!("Could not checkpoint library import: {error}"); }
        self.state.scan_active.store(false, Ordering::Release);
        self.state.event_tx.emit(LibraryEvent::ScanComplete);
    }
}

/// Filesystem discovery fills this batch without holding a database write lock.
#[derive(Default)]
struct ScanBatch {
    work_revision: Option<i64>,
    directories: Vec<(DirId, DirId, String)>,
    paths: Vec<(DirId, String)>,
    seen: Vec<(DirId, String, u64)>,
}

impl ScanBatch {
    fn commit(&mut self, conn: &mut rusqlite::Connection, scan_id: i64) -> rusqlite::Result<()> {
        if self.directories.is_empty() && self.paths.is_empty() && self.seen.is_empty() {
            return Ok(());
        }
        let transaction = client_library::schema::library_write_transaction(conn)?;
        if self.work_revision.is_some_and(|revision| file_work_revision(&transaction).ok() != Some(revision)) {
            return Err(rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY), Some("file commands changed during scan".into())));
        }
        for (id, parent, name) in &self.directories {
            // Discovery does not lock out user edits. Never resurrect a directory
            // (or its children) deleted while this batch was being collected.
            let live_parent: bool = transaction.query_row(include_str!("sql/scanner/commit_select.sql"), [parent.to_string()], |row| row.get(0))?;
            let deleted: bool = transaction.query_row(include_str!("sql/scanner/commit_select_2.sql"), [id.to_string()], |row| row.get(0))?;
            if live_parent && !deleted {
                upsert_dir(&transaction, id, parent, name)?;
            }
        }
        for (id, path) in &self.paths {
            transaction.execute(include_str!("sql/scanner/commit_insert.sql"), rusqlite::params![id.to_string(),path])?;
        }
        mark_known_book_dirs_seen(&transaction, scan_id, &self.seen)?;
        for (dir, name, _) in &self.seen {
            transaction.execute(include_str!("sql/scanner/commit_insert_2.sql"), rusqlite::params![dir.to_string(),name,client_library::file_work::placement_path(&transaction,dir,name)?])?;
        }
        transaction.commit()?;
        self.directories.clear();
        self.paths.clear();
        self.seen.clear();
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ScanFileKey {
    dir_id: sync_common::DirId,
    file_name: sync_common::FileName,
    fingerprint: u64,
}

#[derive(Clone)]
struct HashTask {
    work_revision: Option<i64>,
    key: ScanFileKey,
    book_path: PathBuf,
    scan_id: i64,
    metadata_backfill: bool,
}

type BookMetadata = library_model::BookMetadataState;

enum HashResult {
    Ready {
        task: HashTask,
        content_hash: ContentHash,
        checksum: ContentHash,
        size_bytes: u64,
        format: BookFormat,
        metadata: Option<BookMetadata>,
        audiobook: Option<book_metadata::AudiobookMetadata>,
        pdf: Option<pdf_view_common::PdfReaderMetadata>,
        /// The publication's own navigation, carried from the inspection that
        /// already parsed it. Dropping it here is what left a scanned library
        /// with contents in the reader and none on the book's page.
        toc: Vec<library_model::PublicationTocEntry>,
    },
    Failed { task: HashTask, error: String },
}

impl<D: FileDatabase> LibraryScanner<D> {
    pub fn from_root(library_root: impl Into<PathBuf>, db: D, assets: AssetStore, event_tx: LibraryEventSender) -> std::io::Result<Self> {
        let (hash_tx, hash_rx) = crossbeam_channel::bounded::<HashTask>(HASH_QUEUE_CAPACITY);
        let (hash_result_tx, hash_result_rx) = crossbeam_channel::bounded::<HashResult>(HASH_RESULT_QUEUE_CAPACITY);

        let state = Arc::new(ScannerState {
            library_root: library_root.into(),
            db,
            assets,
            park_thread: OnceLock::new(),
            scan_enabled: AtomicBool::new(false),
            // The initial scan is pending even while desktop startup keeps it parked.
            scan_requested: AtomicBool::new(true),
            scan_active: AtomicBool::new(false),
            should_finish: AtomicBool::new(false),
            hash_queue: hash_tx,
            hash_result_queue: hash_result_tx,
            hash_result_rx,
            event_tx: event_tx.clone(),
            progress: Arc::default(),
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

        Ok(Self { state, watcher_thread: Some(watcher_thread), worker_thread: Some(worker_thread), hash_threads })
    }

    pub fn is_scanning(&self) -> bool {
        self.state.scan_requested.load(Ordering::Acquire) || self.state.scan_active.load(Ordering::Acquire)
    }

    pub fn has_started(&self) -> bool { self.state.scan_enabled.load(Ordering::Acquire) }

    pub fn scan_progress(&self) -> crate::ScanProgress {
        *self.state.progress.lock().unwrap()
    }

    pub fn start_scanning(&self) {
        if self.state.scan_enabled.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(thread) = self.state.park_thread.get() {
            thread.unpark();
        }
    }

    /// Returns whether missing placements still need a grace-window rescan.
    fn scan_library(db: &D, state: &Arc<ScannerState<D>>) -> bool {
        state.scan_active.store(true, Ordering::Release);
        let mut activity = ScanActivity { state, successful: false };
        let checkpoint = db.get().and_then(|conn| {
            let signed_in = client_library::sync_state::get_sync_metadata(&conn, "bound_user_id")?.is_some();
            client_library::operations::begin_import(&conn, signed_in).map_err(Into::into)
        });
        if let Err(error) = checkpoint { log::warn!("Could not begin library import: {error}"); }
        *state.progress.lock().unwrap() = crate::ScanProgress::default();
        if !state.assets.placement().root_available() {
            log::warn!("Scanner: library root is unavailable; preserving placements");
            return false;
        }
        state.event_tx.emit(LibraryEvent::ScanStarted);
        let mut conn = match db.get() {
            Ok(c) => c,
            Err(e) => {
                log::error!("Scanner: Failed to get connection from pool: {}", e);
                return false;
            }
        };

        // Read exclusions consistently; scanning never executes filesystem jobs.
        let pending = match conn.unchecked_transaction().and_then(|tx| PendingScanWork::read(&tx)) {
            Ok(pending) => pending,
            Err(error) => {
                log::warn!("Scanner: cannot read pending paths: {error}");
                return false;
            }
        };

        // Claim this scan's generation up front so both the synchronous walk and
        // the async hash drain stamp the same value; placements left unstamped
        // afterward are reconciled as vanished.
        let scan_id = match client_library::scanner::next_scan_id(&conn) {
            Ok(scan_id) => scan_id,
            Err(error) => {
                log::error!("Scanner: failed to allocate scan generation: {error}");
                return false;
            }
        };
        let known_by_fingerprint = match get_book_dirs_by_local_fingerprint(&conn) {
            Ok(known) => known,
            Err(error) => {
                log::error!("Scanner: failed to preload local fingerprints: {error}");
                return false;
            }
        };
        let audible_current = match conn.prepare(include_str!("sql/scanner/scan_library_select.sql")).and_then(|mut statement| {
            statement.query_map([metadata_contract::audible::POLICY_VERSION], |row| row.get::<_,String>(0))?
                .map(|value| value.map(|value| ContentHash::new(&value))).collect::<Result<HashSet<_>,_>>()
        }) {
            Ok(current) => current,
            Err(error) => { log::error!("Scanner: failed to preload Audible work: {error}"); return false; }
        };
        let tombstoned_book_files = match get_tombstoned_book_file_keys(&conn) {
            Ok(tombstones) => tombstones,
            Err(error) => {
                log::error!("Scanner: failed to preload tombstoned files: {error}");
                return false;
            }
        };
        let mut known_directories = match client_library::browse::get_known_directories_for_scan(&conn) {
            Ok(directories) => directories,
            Err(error) => {
                log::error!("Scanner: failed to preload directories: {error}");
                return false;
            }
        };
        let library_root = state.library_root.as_path().to_path_buf();
        // Do a cheap discovery pass before hashing or writing books. This gives
        // the UI a stable denominator and makes the scan visibly about the
        // folder index first, rather than an indeterminate stream of files.
        let mut discovered_books = 0_u64;
        let mut discovery = WalkDir::new(&library_root).follow_links(false).min_depth(1).into_iter();
        while let Some(entry) = discovery.next() {
            let Ok(entry) = entry else { continue };
            if entry.file_name().to_string_lossy().starts_with('.') {
                if entry.file_type().is_dir() { discovery.skip_current_dir(); }
                continue;
            }
            let Ok(relative) = entry.path().strip_prefix(&library_root) else { continue };
            if pending.excludes(relative) {
                if entry.file_type().is_dir() { discovery.skip_current_dir(); }
                continue;
            }
            if entry.file_type().is_file() && Self::is_supported_book_file(entry.path())
                && entry.metadata().is_ok_and(|metadata| metadata.len() > 0) { discovered_books += 1; }
        }
        state.progress.lock().unwrap().total = discovered_books;
        state.event_tx.emit(LibraryEvent::TransferStatusChanged);
        let mut walkdir = WalkDir::new(state.library_root.as_path()).follow_links(false).min_depth(1).into_iter();
        let root_dir_id = sync_common::ROOT_DIR_ID;
        let mut uuid_stack = vec![root_dir_id.clone()];
        let mut seen_dir_ids = HashSet::from([root_dir_id]);
        let mut suppressed_dir_ids = HashSet::new();
        let mut readable_scan = known_directories.iter().all(|(id, directory)| {
            if let client_library::browse::KnownDirectoryState::Live(Some(path)) = &directory.state {
                if pending.excludes(Path::new(path.as_str())) {
                    return true;
                }
                let path = library_root.join(path.as_str());
                if path.is_dir() {
                    return fs::read_to_string(path.join(".biblos_uuid")).is_ok_and(|marker| marker.trim() == id.to_string());
                }
            }
            true
        });
        let mut batch_entries = 0_usize;
        let mut pending_hash_tasks = Vec::new();
        let mut batch = ScanBatch { work_revision: Some(pending.revision), ..ScanBatch::default() };
        while let Some(next_entry) = walkdir.next() {
            if batch_entries >= SCAN_DB_BATCH_SIZE || pending_hash_tasks.len() >= HASH_QUEUE_CAPACITY {
                if let Err(error) = batch.commit(&mut conn, scan_id) {
                    log::warn!("Scanner: failed to commit scan batch: {error}");
                    return error.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseBusy);
                }
                match Self::process_hash_tasks(state, std::mem::take(&mut pending_hash_tasks)) {
                    Ok(all_readable) => readable_scan &= all_readable,
                    Err(()) => return file_work_revision(&conn).is_ok_and(|revision| revision != pending.revision),
                }
                batch_entries = 0;
            }
            let entry = match next_entry {
                Ok(entry) => entry,
                Err(error) => {
                    readable_scan = false;
                    log::warn!("Scanner: skipping unreadable entry: {error}");
                    continue;
                }
            };
            batch_entries += 1;
            if entry.file_type().is_symlink() {
                readable_scan = false;
            }
            if entry.file_name().to_string_lossy().starts_with('.') {
                if entry.file_type().is_dir() {
                    walkdir.skip_current_dir();
                }
                continue;
            }
            let relative = match entry.path().strip_prefix(&library_root) {
                Ok(relative) => relative,
                Err(error) => {
                    readable_scan = false;
                    log::warn!("Scanner: path escaped library root: {error}");
                    if entry.file_type().is_dir() {
                        walkdir.skip_current_dir();
                    }
                    continue;
                }
            };
            if pending.excludes(relative) {
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
                return false;
            };

            if entry.file_type().is_dir() {
                let Some(raw_name) = entry.file_name().to_str() else {
                    readable_scan = false;
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
                            readable_scan = false;
                            log::warn!("Scanner: invalid directory marker {}: {error}", uuid_path.display());
                            walkdir.skip_current_dir();
                            continue;
                        }
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        let dir_id = DirId::new_v4();
                        if let Err(error) = fs::write(&uuid_path, dir_id.to_string()) {
                            readable_scan = false;
                            log::error!("Scanner: cannot create directory marker {}: {error}", uuid_path.display());
                            walkdir.skip_current_dir();
                            continue;
                        }
                        dir_id
                    }
                    Err(error) => {
                        readable_scan = false;
                        log::warn!("Scanner: cannot read directory marker {}: {error}", uuid_path.display());
                        walkdir.skip_current_dir();
                        continue;
                    }
                };
                let parent_is_suppressed = suppressed_dir_ids.contains(&parent_dir_id);
                match known_directories.get(&dir_id).map(|directory| &directory.state) {
                    Some(client_library::browse::KnownDirectoryState::Deleted) => {
                        seen_dir_ids.insert(dir_id.clone());
                        suppressed_dir_ids.insert(dir_id.clone());
                        uuid_stack.push(dir_id);
                        continue;
                    }
                    Some(client_library::browse::KnownDirectoryState::Live(Some(expected_path))) if Path::new(expected_path.as_str()) != relative => {
                        if seen_dir_ids.contains(&dir_id) {
                            dir_id = DirId::new_v4();
                            if let Err(error) = fs::write(&uuid_path, dir_id.to_string()) {
                                readable_scan = false;
                                log::error!("Scanner: cannot replace duplicate directory marker {}: {error}", uuid_path.display());
                                walkdir.skip_current_dir();
                                continue;
                            }
                        }
                    }
                    None if parent_is_suppressed => {
                        seen_dir_ids.insert(dir_id.clone());
                        suppressed_dir_ids.insert(dir_id.clone());
                        uuid_stack.push(dir_id);
                        continue;
                    }
                    Some(client_library::browse::KnownDirectoryState::Live(_)) | None => {}
                }
                if !seen_dir_ids.insert(dir_id.clone()) {
                    dir_id = DirId::new_v4();
                    if let Err(error) = fs::write(&uuid_path, dir_id.to_string()) {
                        readable_scan = false;
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
                    batch.directories.push((dir_id, parent_dir_id, directory_name.clone()));
                    let Some(relative) = relative.to_str() else {
                        log::error!("Scanner: persisted directory has a non-UTF-8 relative path: {}", entry.path().display());
                        return false;
                    };
                    let relative_path = match crate::RelativeDirPath::parse(relative) {
                        Ok(path) => path,
                        Err(error) => {
                            log::error!("Scanner: persisted directory has an invalid relative path {}: {error}", entry.path().display());
                            return false;
                        }
                    };
                    known_directories.insert(dir_id.clone(), client_library::browse::KnownDirectory { parent_id: parent_dir_id.clone(), name: directory_name.clone(), state: client_library::browse::KnownDirectoryState::Live(Some(relative_path)) });
                }
                batch.paths.push((dir_id, relative.to_string_lossy().into_owned()));
                uuid_stack.push(dir_id);
            } else if entry.file_type().is_file() && Self::is_supported_book_file(entry.path()) {
                let Some(raw_file_name) = entry.file_name().to_str() else {
                    readable_scan = false;
                    log::warn!("Scanner: skipping EPUB with a non-UTF-8 name: {}", entry.path().display());
                    continue;
                };
                let file_name = raw_file_name.to_owned();
                if relative.to_str().and_then(|path| crate::RelativeBookPath::parse(path).ok()).is_none() {
                    readable_scan = false;
                    log::warn!("Scanner: skipping EPUB with an invalid relative path: {}", entry.path().display());
                    continue;
                }
                if tombstoned_book_files.contains(&(parent_dir_id, file_name.clone())) {
                    continue;
                }
                if suppressed_dir_ids.contains(&parent_dir_id) {
                    continue;
                }
                let file_hash = match local_file_fingerprint(entry.path()) {
                    Ok(file_hash) => file_hash,
                    Err(error) => {
                        readable_scan = false;
                        log::warn!("Scanner: cannot fingerprint {}: {error}", entry.path().display());
                        continue;
                    }
                };
                if entry.metadata().is_ok_and(|metadata| metadata.len() == 0) {
                    // An existing placement is still present, even if a writer has
                    // temporarily truncated it. Do not publish or queue empty bytes.
                    batch.seen.push((parent_dir_id, file_name, file_hash));
                    continue;
                }
                let book_dir = known_by_fingerprint.get(&file_hash).cloned();
                let metadata_backfill = match book_dir {
                    // A known fast hash identifies a file already indexed by this
                    // library. It needs neither content hashing nor another
                    // placement; the existing scan entry remains authoritative.
                    Some(known) if (!known.description_missing || Self::book_format_from_path(entry.path()) == Some(BookFormat::Pdf)) && known.dir_id == parent_dir_id && known.file_name == file_name
                        && (Self::book_format_from_path(entry.path()) != Some(BookFormat::M4b) || audible_current.contains(&known.content_hash)) => {
                        // Skipping content inspection must not make the existing
                        // row look stale to this scan's cleanup pass. Accumulate
                        // these observations so one UPDATE marks the whole batch.
                        batch.seen.push((known.dir_id, known.file_name, file_hash));
                        state.progress.lock().unwrap().succeeded += 1;
                        state.event_tx.emit(LibraryEvent::TransferStatusChanged);
                        continue;
                    }
                    Some(known) => known.description_missing,
                    None => false,
                };
                // Unknown files need a portable content hash. Known files with
                // incomplete metadata currently share this worker path, but are
                // marked explicitly so their metadata may be backfilled without
                // overwriting edited metadata for an unrelated mtime-only change.
                pending_hash_tasks.push(HashTask { work_revision: Some(pending.revision), key: ScanFileKey { dir_id: parent_dir_id, file_name, fingerprint: file_hash }, book_path: entry.path().to_path_buf(), scan_id, metadata_backfill });
            }
        }
        if let Err(error) = batch.commit(&mut conn, scan_id) {
            log::warn!("Scanner: failed to commit final scan batch: {error}");
            return error.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseBusy);
        }
        match Self::process_hash_tasks(state, pending_hash_tasks) {
            Ok(all_readable) => readable_scan &= all_readable,
            Err(()) => return file_work_revision(&conn).is_ok_and(|revision| revision != pending.revision),
        }

        // Unmounts, unreadable paths and interrupted scans never imply deletion.
        if !readable_scan || !state.assets.placement().root_available() || state.should_finish.load(Ordering::Acquire) {
            return false;
        }
        let finish = (|| -> rusqlite::Result<bool> {
            let tx = client_library::schema::library_write_transaction(&mut conn)?;
            if file_work_revision(&tx)? != pending.revision {
                return Ok(true);
            }
            pending.protect_placements(&tx, scan_id)?;
            client_library::scanner::delete_vanished_placements_in_tx(&tx, scan_id)?;
            let rescan = client_library::scanner::count_pending_vanish_candidates(&tx, scan_id)? > 0;
            tx.commit()?;
            Ok(rescan)
        })();
        match finish {
            Ok(rescan) => { activity.successful = true; return rescan; },
            Err(error) => log::warn!("Scanner: cannot finish scan: {error}"),
        }
        false
    }

    /// The walk has committed before entering this phase. At most one bounded
    /// batch is in flight; the coordinator persists results while hashing continues.
    /// File failures return Ok(false): continue walking, but do not reconcile deletions.
    /// Interrupted or unpersisted batches return Err and stop the walk.
    fn process_hash_tasks(state: &Arc<ScannerState<D>>, tasks: Vec<HashTask>) -> Result<bool, ()> {
        const MAX_PERSIST_BATCH: usize = 64;
        assert!(tasks.len() <= HASH_QUEUE_CAPACITY);
        let mut keys = HashSet::new();
        let mut remaining = 0;
        for task in tasks {
            if state.should_finish.load(Ordering::Acquire) {
                return Err(());
            }
            if !keys.insert(task.key.clone()) {
                continue;
            }
            // The previous batch was fully drained, so this entire batch fits
            // in the task queue even before any worker receives a task.
            if state.hash_queue.send(task).is_err() {
                return Err(());
            }
            remaining += 1;
        }
        let mut succeeded = true;
        let mut all_readable = true;
        while remaining > 0 {
            if state.should_finish.load(Ordering::Acquire) {
                return Err(());
            }
            let first = match state.hash_result_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(result) => result,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Err(()),
            };
            let batch = result_batch::collect(&state.hash_result_rx, first, remaining.min(MAX_PERSIST_BATCH));
            remaining -= batch.len();
            for result in &batch {
                if let HashResult::Failed { task, error } = result {
                    log::warn!("Scanner: failed to hash {}: {error}", task.book_path.display());
                    all_readable = false;
                }
            }
            {
                let mut progress = state.progress.lock().unwrap();
                progress.succeeded += batch.iter().filter(|result| matches!(result, HashResult::Ready { .. })).count() as u64;
                progress.failed += batch.iter().filter(|result| matches!(result, HashResult::Failed { .. })).count() as u64;
            }
            state.event_tx.emit(LibraryEvent::TransferStatusChanged);
            if batch.iter().any(|result| matches!(result, HashResult::Ready { .. })) {
                succeeded &= Self::persist_ready_hashes(state, &batch);
            }
            // Drain even after a failure so the next scan cannot consume stale
            // results, and hashing workers cannot block on a full result queue.
        }
        if succeeded && !state.should_finish.load(Ordering::Acquire) { Ok(all_readable) } else { Err(()) }
    }
}

impl<D: FileDatabase> Drop for LibraryScanner<D> {
    fn drop(&mut self) {
        self.state.should_finish.store(true, Ordering::Release);
        self.state.scan_enabled.store(true, Ordering::Release);
        self.state.scan_requested.store(true, Ordering::Release);
        if let Some(thread) = self.state.park_thread.get() {
            thread.unpark();
        }
        if let Some(thread) = self.worker_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.watcher_thread.take() {
            let _ = thread.join();
        }
        for thread in self.hash_threads.drain(..) {
            let _ = thread.join();
        }
    }
}

impl<D: FileDatabase> LibraryScanner<D> {
    fn hash_worker_loop(state: Arc<ScannerState<D>>, hash_rx: crossbeam_channel::Receiver<HashTask>) {
        loop {
            if state.should_finish.load(Ordering::Acquire) {
                break;
            }
            let mut task = match hash_rx.recv_timeout(Duration::from_millis(250)) {
                Ok(task) => task,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            };

            let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if super::asset_store::import_in_progress(&task.book_path) {
                    Self::request_scan(&state);
                    return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "book is still being imported; retry"));
                }
                // A local library is a catalogue of the user's files, not an
                // upload trust boundary. Full reader/upload validation rejects
                // many otherwise readable legacy publications (for example
                // EPUB page-template resources and PDFs containing URI actions),
                // which made those files disappear from the library entirely.
                // Keep strict validation at import/upload/open boundaries and
                // let the scanner index every supported local file extension.
                let format = Self::book_format_from_path(&task.book_path).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "unsupported book extension"))?;
                let content_hash = ensure_publication_identity(&task.book_path)?;
                // Embedding changes file metadata. Cache the resulting version,
                // not the pre-embedding fingerprint collected by directory walking.
                task.key.fingerprint = local_file_fingerprint(&task.book_path)?;
                let mut hasher = blake3::Hasher::new();
                hasher.update_reader(fs::File::open(&task.book_path)?)?;
                let checksum = ContentHash::new(hasher.finalize().to_hex().as_str());
                let size_bytes = hasher.count();
                if size_bytes == 0 {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "publication is empty"));
                }
                let (metadata, audiobook, pdf, toc) = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| book_metadata::inspect_book_for_catalogue(&task.book_path, format))) {
                    Ok(inspection) => (Some(inspection.metadata), inspection.audiobook, inspection.pdf, inspection.toc),
                    Err(_) => {
                        log::warn!("Skipping metadata parser panic for {}", task.book_path.display());
                        (None, None, None, Vec::new())
                    }
                };
                if local_file_fingerprint(&task.book_path)? != task.key.fingerprint {
                    Self::request_scan(&state);
                    return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "publication changed during metadata inspection; retry"));
                }
                Ok::<_, std::io::Error>((content_hash, checksum, size_bytes, format, metadata, audiobook, pdf, toc))
            })) {
                Ok(Ok((content_hash, checksum, size_bytes, format, metadata, audiobook, pdf, toc))) => HashResult::Ready { task, content_hash, checksum, size_bytes, format, metadata, audiobook, pdf, toc },
                Ok(Err(error)) => HashResult::Failed { task, error: error.to_string() },
                Err(error) => {
                    let message = error.downcast_ref::<&str>().copied().or_else(|| error.downcast_ref::<String>().map(String::as_str)).unwrap_or("unknown panic");
                    HashResult::Failed { task, error: format!("hash worker panicked: {message}") }
                }
            };
            let mut result = result;
            loop {
                if state.should_finish.load(Ordering::Acquire) {
                    return;
                }
                match state.hash_result_queue.send_timeout(result, Duration::from_millis(250)) {
                    Ok(()) => break,
                    Err(crossbeam_channel::SendTimeoutError::Timeout(pending)) => result = pending,
                    Err(crossbeam_channel::SendTimeoutError::Disconnected(_)) => return,
                }
            }
        }
    }

    fn persist_ready_hashes(state: &Arc<ScannerState<D>>, batch: &[HashResult]) -> bool {
        let leases = batch
            .iter()
            .filter_map(|item| match item {
                HashResult::Ready { content_hash, .. } => Some(state.assets.lease_book_blocking(content_hash)),
                _ => None,
            })
            .collect::<Result<Vec<_>, _>>();
        let _leases = match leases {
            Ok(leases) => leases,
            Err(error) => {
                log::error!("Scanner could not lease publication assets: {error}");
                return false;
            }
        };
        // Verify the observed files under their leases before publishing their
        // metadata. The library files themselves are the publication storage.
        let mut missing_covers = HashSet::new();
        for item in batch {
            let HashResult::Ready { task, content_hash, .. } = item else { continue };
            match fs::File::open(&task.book_path).and_then(|mut file| publication_identity::identify(&mut file)) {
                Ok(actual) if actual == *content_hash && local_file_fingerprint(&task.book_path).is_ok_and(|fingerprint| fingerprint == task.key.fingerprint) => {}
                Ok(_) => {
                    Self::request_scan(state);
                    log::error!("Scanner: publication changed after hashing: {}", task.book_path.display());
                    return false;
                }
                Err(error) => {
                    log::error!("Scanner: failed to read {}: {error}", task.book_path.display());
                    return false;
                }
            }
            if !state.assets.thumbnail_set_exists(content_hash).unwrap_or(false) {
                missing_covers.insert(*content_hash);
            }
        }
        let mut conn = match state.db.get() {
            Ok(conn) => conn,
            Err(error) => {
                log::error!("Scanner: failed to get DB connection: {error}");
                return false;
            }
        };
        // One book has one catalogued filename per folder. Keep that file when
        // duplicate identities coexist, otherwise successive scans would flip
        // between unchanged revisions and manufacture new upload jobs. All file
        // checks remain outside the writer reservation; a vanished path can be
        // replaced by its externally renamed counterpart.
        let mut selected = std::collections::HashMap::new();
        for item in batch {
            let HashResult::Ready { task, content_hash, .. } = item else { continue };
            let key = (task.key.dir_id, *content_hash);
            if selected.contains_key(&key) { continue; }
            let existing = client_library::placements::live_file_name(&conn, &task.key.dir_id, content_hash);
            let chosen = match existing {
                Ok(name) if name != task.key.file_name => {
                    let existing_path = task.book_path.parent().unwrap().join(&name);
                    if fs::File::open(existing_path).and_then(|mut file| publication_identity::identify(&mut file)).is_ok_and(|identity| identity == *content_hash) { name }
                    else { task.key.file_name.clone() }
                }
                Ok(name) => name,
                Err(rusqlite::Error::QueryReturnedNoRows) => task.key.file_name.clone(),
                Err(error) => { log::error!("Scanner: failed to select publication placement: {error}"); return false; }
            };
            selected.insert(key, chosen);
        }
        let tx = match client_library::schema::library_write_transaction(&mut conn) {
            Ok(tx) => tx,
            Err(error) => {
                log::error!("Scanner: failed to start transaction: {error}");
                return false;
            }
        };
        for item in batch {
            let HashResult::Ready { task, content_hash, checksum, size_bytes, format, metadata, audiobook, pdf, toc } = item else { continue };
            if selected.get(&(task.key.dir_id, *content_hash)) != Some(&task.key.file_name) { continue; }
            if task.work_revision.is_some_and(|revision| file_work_revision(&tx).ok() != Some(revision)) {
                return false;
            }
            match client_library::browse::is_book_file_tombstoned(&tx, &task.key.dir_id, &task.key.file_name) {
                Ok(true) => continue,
                Ok(false) => match client_library::browse::get_known_directory_scan_state(&tx, &task.key.dir_id) {
                    Ok(Some(client_library::browse::KnownDirectoryState::Deleted) | None) => continue,
                    Ok(Some(client_library::browse::KnownDirectoryState::Live(_))) => {}
                    Err(error) => {
                        log::error!("Scanner: failed to check directory state: {error}");
                        return false;
                    }
                },
                Err(error) => {
                    log::error!("Scanner: failed to check tombstone state: {error}");
                    return false;
                }
            }
            let new_book = match tx.query_row(include_str!("sql/scanner/persist_ready_hashes_select_2.sql"), [content_hash.as_str()], |row| row.get::<_, bool>(0)) {
                Ok(value) => value,
                Err(error) => { log::error!("Scanner: could not check new book: {error}"); return false; }
            };
            let already_indexed = match client_library::scanner::book_has_live_placement(&tx, &task.key.dir_id, content_hash.clone()) {
                Ok(value) => value,
                Err(error) => {
                    log::error!("Scanner: failed to check duplicate placement: {error}");
                    return false;
                }
            };
            let metadata_result = match (already_indexed, task.metadata_backfill, metadata) {
                (true, false, _) => Ok(()),
                (_, _, Some(metadata)) => client_library::metadata::insert_scanned_book_metadata(&tx, content_hash.clone(), metadata, *format),
                (_, _, None) => insert_book(&tx, content_hash.clone(), *format),
            };
            if let Err(error) = metadata_result.and_then(|_| insert_local_book_dir(&tx, &task.key.dir_id, content_hash.clone(), &task.key.file_name, task.key.fingerprint, task.scan_id)) {
                log::error!("Scanner: database batch failed: {error}");
                return false;
            }
            // Stored whenever this file was inspected, including for a book the
            // library already knows: the rule above that leaves an indexed book's
            // metadata alone is there to protect edits, and a navigation document
            // is the publication's, not the reader's.
            if metadata.is_some() {
                if let Some((source, entries)) = client_library::toc::inspected_navigation(*format, toc, pdf.as_ref()) {
                    if let Err(error) = client_library::toc::upsert_book_toc(&tx, content_hash, source, &entries) {
                        log::error!("Scanner: table of contents commit failed: {error}");
                        return false;
                    }
                }
            }
            if let Some(pdf) = pdf {
                if let Err(error) = client_library::pdf_metadata::upsert(&tx, content_hash, checksum, pdf) {
                    log::error!("Scanner: PDF metadata commit failed: {error}");
                    return false;
                }
            }
            if *size_bytes > 0 {
                if let Err(error) = client_library::assets::observe_local_placement_publication(&tx, &task.key.dir_id, content_hash, checksum, *size_bytes) {
                    log::error!("Scanner: failed to record local upload intent: {error}");
                    return false;
                }
            }
            let relative = match client_library::file_work::placement_path(&tx, &task.key.dir_id, &task.key.file_name) {
                Ok(relative) => relative,
                Err(error) => {
                    log::error!("Scanner: invalid publication path: {error}");
                    return false;
                }
            };
            if let Err(error) = tx.execute(
                include_str!("../asset_store/sql/asset_store/publish_insert.sql"),
                rusqlite::params![content_hash.as_str(), task.key.dir_id.to_string(), relative],
            ) {
                log::error!("Scanner: failed to remember publication path: {error}");
                return false;
            }
            // Audiobook chapters and recording evidence describe file bytes,
            // so refresh them after tag edits even when bibliographic metadata
            // has already been enriched or edited independently in the library.
            if let Some(audiobook) = audiobook {
                if let Err(error) = client_library::audiobooks::upsert_audiobook_metadata(&tx, content_hash, audiobook) {
                    log::error!("Scanner: failed to persist audiobook metadata: {error}");
                    return false;
                }

            }
            if new_book && missing_covers.contains(content_hash) {
                let remote = tx.query_row(include_str!("sql/scanner/persist_ready_hashes_select_3.sql"), [content_hash.as_str()], |row| row.get::<_, bool>(0));
                let result = remote.and_then(|remote| {
                    if remote { client_library::assets::enqueue_asset_work(&tx, content_hash) } else { client_library::thumbnail_jobs::request_missing_thumbnail(&tx, content_hash) }
                });
                if let Err(error) = result {
                    log::error!("Scanner: failed to queue cover recovery: {error}");
                    return false;
                }
            }
        }
        if let Err(error) = tx.commit() {
            log::error!("Scanner: failed to commit transaction: {error}");
            return false;
        }
        true
    }

    fn worker_loop(state: Arc<ScannerState<D>>) {
        let _ = state.park_thread.set(thread::current());
        let mut last_scan_time: Option<Instant> = None;
        let mut rescan_at: Option<Instant> = None;
        loop {
            if state.should_finish.load(Ordering::Acquire) {
                return;
            }
            if !state.scan_enabled.load(Ordering::Acquire) {
                thread::park();
                continue;
            }
            let now = Instant::now();
            if state.scan_requested.load(Ordering::Acquire) || rescan_at.is_some_and(|deadline| deadline <= now) {
                if let Some(allowed_at) = last_scan_time.map(|last| last + Duration::from_millis(500)) {
                    if now < allowed_at {
                        thread::park_timeout(allowed_at - now);
                        continue;
                    }
                }
                // Keep the public state busy while handing queued work to the scan.
                state.scan_active.store(true, Ordering::Release);
                state.scan_requested.store(false, Ordering::Release);
                let needs_rescan = Self::scan_library(&state.db, &state);
                let finished_at = Instant::now();
                last_scan_time = Some(finished_at);
                rescan_at = needs_rescan.then_some(finished_at + RESCAN_DELAY);
            } else if let Some(deadline) = rescan_at {
                thread::park_timeout(deadline.saturating_duration_since(now));
            } else {
                thread::park();
            }
        }
    }

    #[cfg(feature = "watcher")]
    fn watcher_loop(state: Arc<ScannerState<D>>) {
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
        Self::watcher_event_loop(&state, &event_rx);
        drop(watcher);
    }

    #[cfg(feature = "watcher")]
    fn watcher_event_loop(state: &ScannerState<D>, event_rx: &std::sync::mpsc::Receiver<notify::Result<Event>>) {
        let debounce = Duration::from_millis(500);
        let mut last_relevant_event: Option<Instant> = None;
        loop {
            if state.should_finish.load(Ordering::Acquire) {
                break;
            }

            // Do not make dispatch depend on the entire notify channel becoming
            // quiet. SQLite WAL and other ignored internal files can generate a
            // continuous stream of events. They must not starve a pending book
            // event after its own debounce period has elapsed.
            if last_relevant_event.is_some_and(|event| event.elapsed() >= debounce) {
                Self::request_scan(state);
                last_relevant_event = None;
            }
            let wait = last_relevant_event.map_or(debounce, |event| debounce.saturating_sub(event.elapsed()));
            match event_rx.recv_timeout(wait) {
                Ok(Ok(event)) => {
                    if Self::should_trigger_scan(&event) {
                        last_relevant_event = Some(Instant::now());
                    }
                }
                Ok(Err(e)) => log::error!("Watch error: {:?}", e),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if last_relevant_event.take().is_some() {
                        Self::request_scan(state);
                    }
                }
                Err(e) => {
                    log::error!("Channel error: {:?}", e);
                    break;
                }
            }
        }
    }

    #[cfg(not(feature = "watcher"))]
    fn watcher_loop(_state: Arc<ScannerState<D>>) {}

    #[cfg(feature = "watcher")]
    fn should_trigger_scan(event: &Event) -> bool {
        if matches!(event.kind, notify::EventKind::Access(_) | notify::EventKind::Other) && !event.need_rescan() {
            return false;
        }
        if event.paths.is_empty() {
            return true;
        }
        event.paths.iter().any(|path| Self::is_relevant_path(path))
    }

    fn request_scan(state: &ScannerState<D>) {
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


#[cfg(test)]
mod tests {
    mod pdf_streaming_tests;

    use super::*;
    use crate::test_database::TestDatabase as Database;
    type LibraryScanner = super::LibraryScanner<Database>;
    type ScannerState = super::ScannerState<Database>;
    use std::{fs, thread, time::Duration};
    use tempfile::tempdir;
    use test_support::{fixture_content_hash, fixture_dir_id};

    fn scanner_epub(title: &str) -> Vec<u8> {
        scanner_epub_bytes(title, false)
    }

    /// The same publication with an EPUB 3 navigation document, for the scan
    /// that has to keep one.
    fn scanner_epub_with_navigation(title: &str) -> Vec<u8> {
        scanner_epub_bytes(title, true)
    }

    fn scanner_epub_bytes(title: &str, navigated: bool) -> Vec<u8> {
        use std::io::{Cursor, Write};
        let nav_item = if navigated { r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"# } else { "" };
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, contents) in [
            ("mimetype", "application/epub+zip".into()),
            ("META-INF/container.xml", r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.into()),
            (
                "book.opf",
                format!(
                    r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{title}</dc:title><dc:creator>JRR Tolkien</dc:creator></metadata><manifest>{nav_item}<item id="page" href="page.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="page"/></spine></package>"#
                ),
            ),
            (
                "nav.xhtml",
                r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="page.xhtml">Chapter One</a></li><li><a href="page.xhtml#two">Chapter Two</a></li></ol></nav></body></html>"#.into(),
            ),
            ("page.xhtml", "<html xmlns='http://www.w3.org/1999/xhtml'><body>Test</body></html>".into()),
        ] {
            zip.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(contents.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn test_scanner_state(library_root: &Path) -> (Arc<ScannerState>, crossbeam_channel::Receiver<HashTask>, async_channel::Receiver<LibraryEvent>) {
        let db_path = library_root.join(APP_HIDDEN_LIBRARY_DB_PATH);
        fs::create_dir_all(db_path.parent().expect("library db parent exists")).unwrap();

        let db = Database::open(db_path, client_library::schema::configure_connection).unwrap();
        {
            let conn = db.get().unwrap();
            client_library::schema::init_db(&conn).unwrap();
        }

        let (hash_tx, hash_rx) = crossbeam_channel::bounded::<HashTask>(HASH_QUEUE_CAPACITY);
        let (hash_result_tx, hash_result_rx) = crossbeam_channel::bounded::<HashResult>(HASH_RESULT_QUEUE_CAPACITY);
        let (event_tx, event_rx) = crate::events::test_channel();

        let state = Arc::new(ScannerState {
            library_root: library_root.to_path_buf(),
            assets: AssetStore::open(library_root.to_str().unwrap()).unwrap().with_database_path(db.path().to_path_buf()),
            db,
            park_thread: OnceLock::new(),
            scan_enabled: AtomicBool::new(true),
            scan_requested: AtomicBool::new(true),
            scan_active: AtomicBool::new(true),
            progress: Arc::default(),
            should_finish: AtomicBool::new(false),
            hash_queue: hash_tx,
            hash_result_queue: hash_result_tx,
            hash_result_rx,
            event_tx,
        });

        (state, hash_rx, event_rx)
    }

    #[test]
    fn discovered_batch_is_unwritten_until_commit_and_rolls_back_on_failure() {
        let root = tempdir().unwrap();
        let library_root = root.path().join("Books");
        fs::create_dir_all(&library_root).unwrap();

        let (state, _hash_rx, _events) = test_scanner_state(&library_root);
        let mut conn = state.db.get().unwrap();
        let parent = DirId::new_v4();
        let child = DirId::new_v4();
        let mut batch = ScanBatch::default();
        batch.directories.push((parent, sync_common::ROOT_DIR_ID, "Parent".into()));
        batch.directories.push((child, parent, "Child".into()));
        // Another connection can write while discovery has uncommitted results.
        let other = state.db.get().unwrap();
        other.busy_timeout(Duration::ZERO).unwrap();
        client_library::sync_state::set_sync_metadata(&other, "independent-write", "1").unwrap();
        assert_eq!(other.query_row(include_str!("sql/scanner/discovered_batch_is_unwritten_until_commit_and_rolls_back_on_failure_select.sql"), [parent.to_string()], |row| row.get::<_, i64>(0)).unwrap(), 0);
        conn.execute_batch(include_str!("sql/scanner/discovered_batch_is_unwritten_until_commit_and_rolls_back_on_failure_create.sql")).unwrap();
        assert!(batch.commit(&mut conn, 1).is_err());
        assert_eq!(other.query_row(include_str!("sql/scanner/discovered_batch_is_unwritten_until_commit_and_rolls_back_on_failure_select.sql"), [parent.to_string()], |row| row.get::<_, i64>(0)).unwrap(), 0, "the parent must roll back with the failed child");
        conn.execute_batch(include_str!("sql/scanner/discovered_batch_is_unwritten_until_commit_and_rolls_back_on_failure_drop.sql")).unwrap();
        batch.commit(&mut conn, 1).unwrap();
        assert_eq!(other.query_row(include_str!("sql/scanner/discovered_batch_is_unwritten_until_commit_and_rolls_back_on_failure_select_3.sql"), [parent.to_string(), child.to_string()], |row| row.get::<_, i64>(0)).unwrap(), 2);
    }

    #[test]
    fn discovered_batch_does_not_resurrect_a_concurrently_deleted_directory() {
        let root = tempdir().unwrap();
        let library_root = root.path().join("Books");
        fs::create_dir_all(&library_root).unwrap();

        let (state, _hash_rx, _events) = test_scanner_state(&library_root);
        let mut conn = state.db.get().unwrap();
        let parent = DirId::new_v4();
        let child = DirId::new_v4();
        let mut initial = ScanBatch::default();
        initial.directories.push((parent, sync_common::ROOT_DIR_ID, "Parent".into()));
        initial.commit(&mut conn, 1).unwrap();
        let mut discovered = ScanBatch::default();
        discovered.directories.push((parent, sync_common::ROOT_DIR_ID, "Renamed on disk".into()));
        discovered.directories.push((child, parent, "Child".into()));
        state.db.get().unwrap().execute(include_str!("sql/scanner/discovered_batch_does_not_resurrect_a_concurrently_deleted_directory_update.sql"), [parent.to_string()]).unwrap();
        discovered.commit(&mut conn, 2).unwrap();
        assert_eq!(conn.query_row(include_str!("sql/scanner/discovered_batch_does_not_resurrect_a_concurrently_deleted_directory_select.sql"), [parent.to_string(), child.to_string()], |row| row.get::<_, i64>(0)).unwrap(), 0);
    }

    #[test]
    fn fresh_scan_completes_while_background_cleanup_is_pending() {
        let root = tempdir().unwrap();
        let library_root = root.path().join("Books");
        fs::create_dir_all(&library_root).unwrap();

        let (state, _hash_rx, events) = test_scanner_state(&library_root);
        state.scan_requested.store(false, Ordering::Release);
        client_library::sync_state::set_sync_metadata(&state.db.get().unwrap(), "local_asset_maintenance_pending", "1").unwrap();
        assert!(!LibraryScanner::scan_library(&state.db, &state), "background cleanup must not defer scanning");
        assert!(!state.scan_active.load(Ordering::Acquire));
        let mut completed = false;
        while let Ok(event) = events.try_recv() {
            completed |= matches!(event, LibraryEvent::ScanComplete);
        }
        assert!(completed, "a real completed scan settles the library");
    }

    #[test]
    fn initial_scan_stays_pending_until_deferred_work_finishes() {
        let root = tempdir().unwrap();
        let library_root = root.path().join("library");
        fs::create_dir_all(&library_root).unwrap();

        let (state, _hash_rx, _) = test_scanner_state(&library_root);
        let (event_tx, events) = crate::events::test_channel();
        let scanner = LibraryScanner::from_root(&library_root, state.db.clone(), state.assets.clone(), event_tx).unwrap();
        assert!(scanner.is_scanning(), "a deferred initial scan is pending, not an empty library");
        assert!(!scanner.state.scan_enabled.load(Ordering::Acquire));
        assert!(events.try_recv().is_err(), "deferred scanning must remain parked");
        scanner.start_scanning();
        await_scan_complete(&events, Duration::from_secs(5));
        assert!(!scanner.is_scanning(), "an empty library is settled only after its scan completes");
    }

    #[test]
    fn pending_hash_identity_includes_placement_not_only_fingerprint() {
        let first = ScanFileKey { dir_id: fixture_dir_id("first"), file_name: "book.epub".to_owned(), fingerprint: 7 };
        let second = ScanFileKey { dir_id: fixture_dir_id("second"), file_name: "book.epub".to_owned(), fingerprint: 7 };
        let mut pending = HashSet::new();
        assert!(pending.insert(first));
        assert!(pending.insert(second));
    }

    fn scanner_with_missing_placement() -> (tempfile::TempDir, LibraryScanner, async_channel::Receiver<LibraryEvent>) {
        let root = tempdir().unwrap();
        let library_root = root.path().join("library");
        fs::create_dir_all(&library_root).unwrap();

        let (state, _hash_rx, events) = test_scanner_state(&library_root);
        {
            let conn = state.db.get().unwrap();
            let hash = fixture_content_hash(1);
            conn.execute(include_str!("sql/scanner/scanner_with_missing_placement_insert.sql"), [hash.as_str()]).unwrap();
            conn.execute(
                include_str!("sql/scanner/scanner_with_missing_placement_insert_2.sql"),
                [hash.as_str()],
            )
            .unwrap();
            client_library::sync_state::set_sync_metadata(&conn, "scan_seq", "1").unwrap();
        }
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::worker_loop(worker_state));
        (root, LibraryScanner { state, worker_thread: Some(worker), watcher_thread: None, hash_threads: Vec::new() }, events)
    }

    fn await_scan_complete(events: &async_channel::Receiver<LibraryEvent>, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            match events.try_recv() {
                Ok(LibraryEvent::ScanComplete) => return,
                Ok(_) => {}
                Err(async_channel::TryRecvError::Closed) => panic!("scan event channel closed"),
                Err(async_channel::TryRecvError::Empty) => {
                    assert!(Instant::now() < deadline, "scan must finish");
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    #[test]
    fn worker_deadline_advances_missing_file_grace_without_watcher_events() {
        let (_root, scanner, events) = scanner_with_missing_placement();
        await_scan_complete(&events, Duration::from_secs(3));
        assert!(client_library::placements::library_trash(&scanner.state.db.get().unwrap()).unwrap().books.is_empty());
        await_scan_complete(&events, RESCAN_DELAY + Duration::from_secs(3));
        assert_eq!(client_library::placements::library_trash(&scanner.state.db.get().unwrap()).unwrap().books.len(), 1);
    }

    #[test]
    fn dropping_scanner_interrupts_its_rescan_deadline() {
        let (_root, scanner, events) = scanner_with_missing_placement();
        await_scan_complete(&events, Duration::from_secs(3));
        let (finished, completion) = std::sync::mpsc::channel();
        let dropper = thread::spawn(move || {
            drop(scanner);
            finished.send(()).unwrap();
        });
        completion.recv_timeout(Duration::from_secs(2)).expect("shutdown must wake the worker instead of waiting for its rescan deadline");
        dropper.join().unwrap();
    }

    #[test]
    fn only_an_explicit_request_rescans_an_enabled_scanner() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let library_root = library_root_path.canonicalize().unwrap();

        let (state, _hash_rx, _event_rx) = test_scanner_state(&library_root);
        state.scan_requested.store(false, Ordering::Release);
        state.scan_active.store(false, Ordering::Release);

        let scanner = LibraryScanner { state: state.clone(), watcher_thread: None, worker_thread: None, hash_threads: Vec::new() };
        scanner.start_scanning();
        assert!(!state.scan_requested.load(Ordering::Acquire));

        LibraryScanner::request_scan(&state);
        assert!(state.scan_requested.load(Ordering::Acquire));
    }

    #[cfg(feature = "watcher")]
    #[test]
    fn reading_a_book_does_not_trigger_another_scan() {
        let book_path = PathBuf::from("library/book.epub");
        let access = Event::new(notify::EventKind::Access(notify::event::AccessKind::Any)).add_path(book_path.clone());
        let modify = Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Any)).add_path(book_path);

        assert!(!LibraryScanner::should_trigger_scan(&access));
        assert!(LibraryScanner::should_trigger_scan(&modify));
    }

    #[cfg(feature = "watcher")]
    #[test]
    fn ignored_event_stream_does_not_starve_pending_scan() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(library_root_path.join(APP_HIDDEN_DIR)).unwrap();
        let library_root = library_root_path.canonicalize().unwrap();

        let (state, _hash_rx, _event_rx) = test_scanner_state(&library_root);
        state.scan_requested.store(false, Ordering::Release);

        let (event_tx, event_rx) = std_channel();
        let watcher_state = Arc::clone(&state);
        let watcher = thread::spawn(move || LibraryScanner::watcher_event_loop(&watcher_state, &event_rx));

        event_tx.send(Ok(Event::new(notify::EventKind::Any).add_path(library_root.join("incoming.epub")))).unwrap();
        let noise_tx = event_tx.clone();
        let hidden_path = library_root.join(APP_HIDDEN_DIR).join("library.db-wal");
        let noise = thread::spawn(move || {
            let until = Instant::now() + Duration::from_millis(900);
            while Instant::now() < until {
                noise_tx.send(Ok(Event::new(notify::EventKind::Any).add_path(hidden_path.clone()))).unwrap();
                thread::sleep(Duration::from_millis(1));
            }
        });

        thread::sleep(Duration::from_millis(700));
        let scan_requested_while_noise_was_active = state.scan_requested.load(Ordering::Acquire);
        noise.join().unwrap();
        state.should_finish.store(true, Ordering::Release);
        drop(event_tx);
        watcher.join().unwrap();

        assert!(scan_requested_while_noise_was_active, "ignored filesystem traffic must not keep a relevant book event pending forever");
    }

    #[test]
    fn pending_cover_discovery_does_not_defer_scan_generation() {
        let root = tempdir().unwrap();
        let library_root = root.path().join("library");
        fs::create_dir_all(&library_root).unwrap();

        let (state, _hash_rx, _event_rx) = test_scanner_state(&library_root);
        let generation = client_library::scanner::next_scan_id(&state.db.get().unwrap()).unwrap();
        client_library::sync_state::set_sync_metadata(&state.db.get().unwrap(), "local_asset_maintenance_pending", "1").unwrap();
        assert!(!LibraryScanner::scan_library(&state.db, &state));
        assert_eq!(client_library::scanner::next_scan_id(&state.db.get().unwrap()).unwrap(), generation + 2);
        assert_eq!(client_library::sync_state::get_sync_metadata(&state.db.get().unwrap(), "local_asset_maintenance_pending").unwrap().as_deref(), Some("1"));
    }

    #[test]
    fn scan_never_deletes_a_tombstoned_file() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let book_path = library_root_path.join("kept.epub");
        fs::write(&book_path, b"locally retained publication").unwrap();
        let library_root = library_root_path.canonicalize().unwrap();

        let (state, _hash_rx, _event_rx) = test_scanner_state(&library_root);
        let content_hash = fixture_content_hash(1);
        {
            let conn = state.db.get().unwrap();
            conn.execute(include_str!("sql/scanner/scan_never_deletes_a_tombstoned_file_insert.sql"), [content_hash.as_str()]).unwrap();
            conn.execute(
                include_str!("sql/scanner/scan_never_deletes_a_tombstoned_file_insert_2.sql"),
                rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), content_hash.as_str()],
            )
            .unwrap();
        }

        LibraryScanner::scan_library(&state.db, &state);

        assert!(book_path.exists());
        assert_eq!(fs::read(book_path).unwrap(), b"locally retained publication");
    }

    #[test]
    fn hash_worker_missing_file_marks_batch_unreadable_and_drains_results() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let library_root = library_root_path.canonicalize().unwrap();

        let (state, hash_rx, _event_rx) = test_scanner_state(&library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));

        let key = ScanFileKey { dir_id: fixture_dir_id("dir-a"), file_name: "missing.epub".to_owned(), fingerprint: 7 };
        let task = HashTask { work_revision: None, key: key.clone(), book_path: library_root_path.join("missing.epub"), scan_id: 1, metadata_backfill: false };
        assert_eq!(LibraryScanner::process_hash_tasks(&state, vec![task]), Ok(false));

        assert!(state.hash_result_rx.is_empty());

        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
    }

    #[tokio::test]
    async fn direct_import_is_not_indexed_by_the_scanner_before_commit() {
        let app_data = tempdir().unwrap();
        let library_root = app_data.path().join("library");
        fs::create_dir_all(&library_root).unwrap();
        let library_root = library_root.canonicalize().unwrap();

        let (state, hash_rx, _events) = test_scanner_state(&library_root);
        let (_, pending) = state.assets.copy_publication("/book.epub".into(), Box::new(std::io::Cursor::new(scanner_epub("Pending"))), vec![]).await.unwrap();
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));
        state.hash_queue.send(HashTask {
            work_revision: None,
            key: ScanFileKey { dir_id: sync_common::ROOT_DIR_ID, file_name: "book.epub".into(), fingerprint: 1 },
            book_path: library_root.join("book.epub"), scan_id: 1, metadata_backfill: false,
        }).unwrap();
        let result = state.hash_result_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
        assert!(matches!(result, HashResult::Failed { error, .. } if error.contains("still being imported")));
        assert!(state.scan_requested.load(Ordering::Acquire));
        drop(pending);
        assert!(!library_root.join("book.epub").exists());
    }

    #[test]
    fn persistence_failure_drains_parallel_results_and_preserves_missing_placements() {
        let app_data = tempdir().unwrap();
        let root = app_data.path().join("library");
        fs::create_dir_all(&root).unwrap();
        for index in 0..70 {
            fs::write(root.join(format!("{index}.epub")), scanner_epub(&format!("Book {index}"))).unwrap();
        }

        let (state, hash_rx, _events) = test_scanner_state(&root);
        let missing_hash = fixture_content_hash(999);
        {
            let conn = state.db.get().unwrap();
            conn.execute(include_str!("sql/scanner/scanner_with_missing_placement_insert.sql"), [missing_hash.as_str()]).unwrap();
            conn.execute(
                include_str!("sql/scanner/scanner_with_missing_placement_insert_2.sql"),
                [missing_hash.as_str()],
            )
            .unwrap();
            client_library::sync_state::set_sync_metadata(&conn, "scan_seq", "10").unwrap();
            conn.execute_batch(include_str!("sql/scanner/persistence_failure_drains_parallel_results_and_preserves_missing_placements_create.sql")).unwrap();
        }
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let state = Arc::clone(&state);
                let rx = hash_rx.clone();
                thread::spawn(move || LibraryScanner::hash_worker_loop(state, rx))
            })
            .collect();
        LibraryScanner::scan_library(&state.db, &state);
        assert!(state.hash_result_rx.is_empty());
        assert!(state.hash_queue.is_empty());
        {
            let conn = state.db.get().unwrap();
            assert!(client_library::placements::library_trash(&conn).unwrap().books.is_empty(), "failed persistence must not infer external deletion");
            conn.execute_batch(include_str!("sql/scanner/persistence_failure_drains_parallel_results_and_preserves_missing_placements_drop.sql")).unwrap();
        }
        LibraryScanner::scan_library(&state.db, &state);
        let count: i64 = state.db.get().unwrap().query_row(include_str!("sql/scanner/persistence_failure_drains_parallel_results_and_preserves_missing_placements_select.sql"), [], |row| row.get(0)).unwrap();
        assert_eq!(count, 70, "the next scan must consume only its own results");
        assert!(state.hash_result_rx.is_empty());
        state.should_finish.store(true, Ordering::Release);
        for worker in workers {
            worker.join().unwrap();
        }
    }

    #[test]
    fn cancellation_releases_a_hash_worker_with_a_full_result_queue() {
        let app_data = tempdir().unwrap();
        let root = app_data.path().join("library");
        fs::create_dir_all(&root).unwrap();

        let (state, hash_rx, _events) = test_scanner_state(&root);
        let task = HashTask { work_revision: None, key: ScanFileKey { dir_id: sync_common::ROOT_DIR_ID, file_name: "missing.epub".into(), fingerprint: 1 }, book_path: root.join("missing.epub"), scan_id: 1, metadata_backfill: false };
        for _ in 0..HASH_RESULT_QUEUE_CAPACITY {
            assert!(state.hash_result_queue.try_send(HashResult::Failed { task: task.clone(), error: "queued result".into() }).is_ok());
        }
        state.hash_queue.send(task).unwrap();
        let worker_state = Arc::clone(&state);
        let (done, completed) = crossbeam_channel::bounded(1);
        let worker = thread::spawn(move || {
            LibraryScanner::hash_worker_loop(worker_state, hash_rx);
            done.send(()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        while !state.hash_queue.is_empty() {
            assert!(Instant::now() < deadline, "worker must receive the queued task");
            thread::yield_now();
        }
        state.should_finish.store(true, Ordering::Release);
        completed.recv_timeout(Duration::from_secs(3)).expect("cancellation must not require draining the full result queue");
        worker.join().unwrap();
        assert!(state.hash_result_queue.is_full());
    }

    #[test]
    fn local_catalogue_keeps_supported_files_that_fail_strict_reader_validation() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        fs::write(library_root_path.join("legacy.epub"), b"legacy publication bytes").unwrap();
        for name in ["empty.pdf", "empty.epub", "empty.m4b"] {
            fs::write(library_root_path.join(name), []).unwrap();
        }
        let library_root = library_root_path.canonicalize().unwrap();

        let (state, hash_rx, _event_rx) = test_scanner_state(&library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));

        LibraryScanner::scan_library(&state.db, &state);

        let conn = state.db.get().unwrap();
        let placements: i64 = conn.query_row(include_str!("sql/scanner/local_catalogue_keeps_supported_files_that_fail_strict_reader_validation_select.sql"), (), |row| row.get(0)).unwrap();
        assert_eq!(placements, 1);
        let books: i64 = conn.query_row(include_str!("../../tests/fixtures/durable_queue_changes_wake_only_their_workers_select.sql"), [], |row| row.get(0)).unwrap();
        assert_eq!(books, 1, "empty files must not create book entries or jobs");
        assert_eq!(state.progress.lock().unwrap().total, 1);
        drop(conn);

        fs::write(library_root.join("legacy.epub"), []).unwrap();
        LibraryScanner::scan_library(&state.db, &state);
        let live: i64 = state.db.get().unwrap().query_row(include_str!("sql/scanner/pending_folder_moves_do_not_block_unrelated_discovery_or_deletions_select_2.sql"), [], |row| row.get(0)).unwrap();
        assert_eq!(live, 1, "temporarily empty files must not delete existing placements");
        assert_eq!(state.progress.lock().unwrap().total, 0);
        fs::write(library_root.join("empty.epub"), b"replacement publication bytes").unwrap();
        LibraryScanner::scan_library(&state.db, &state);
        let books: i64 = state.db.get().unwrap().query_row(include_str!("../../tests/fixtures/durable_queue_changes_wake_only_their_workers_select.sql"), [], |row| row.get(0)).unwrap();
        assert_eq!(books, 2, "a placeholder must be discovered after bytes arrive");

        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
    }

    /// A scan is how most libraries are filled, and the navigation it parses on
    /// the way past is the only copy a book's page ever sees — the reader parses
    /// the file again for itself. Keeping it for the import path alone left every
    /// scanned book claiming to have no contents.
    #[test]
    fn a_scan_keeps_the_navigation_it_parsed() {
        let app_data = tempdir().unwrap();
        let root = app_data.path().join("library");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("navigated.epub"), scanner_epub_with_navigation("Navigated")).unwrap();
        fs::write(root.join("plain.epub"), scanner_epub("Plain")).unwrap();

        let (state, hash_rx, _events) = test_scanner_state(&root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));

        LibraryScanner::scan_library(&state.db, &state);

        let conn = state.db.get().unwrap();
        let scanned = |file: &str| {
            let hash: String = conn
                .query_row(include_str!("sql/scanner/a_scan_keeps_the_navigation_it_parsed_select.sql"), [file], |row| row.get(0))
                .unwrap_or_else(|error| panic!("{file} was not indexed by the scan: {error}"));
            client_library::toc::book_toc(&conn, &ContentHash::new(&hash)).unwrap()
        };
        let navigated = scanned("navigated.epub").expect("a scanned book keeps the navigation it was inspected with");
        assert_eq!(navigated.iter().map(|entry| entry.title.as_str()).collect::<Vec<_>>(), ["Chapter One", "Chapter Two"]);
        // Recorded as inspected-and-empty rather than left absent, which is what
        // stops anything opening the file again hoping for a different answer.
        assert_eq!(scanned("plain.epub"), Some(Vec::new()), "a publication with no navigation records that it has none");
        drop(conn);

        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
    }

    #[test]
    fn scan_backfills_metadata_for_an_existing_placement() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let book_path = library_root_path.join("legacy.epub");
        fs::write(&book_path, scanner_epub("Backfilled title")).unwrap();
        let library_root = library_root_path.canonicalize().unwrap();

        let (state, hash_rx, _event_rx) = test_scanner_state(&library_root);
        let content_hash = ensure_publication_identity(&book_path).unwrap();
        let fingerprint = local_file_fingerprint(&book_path).unwrap();
        {
            let conn = state.db.get().unwrap();
            conn.execute(include_str!("sql/scanner/scan_backfills_metadata_for_an_existing_placement_insert.sql"), [content_hash.as_str()]).unwrap();
            conn.execute(
                include_str!("sql/scanner/scan_backfills_metadata_for_an_existing_placement_insert_2.sql"),
                rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), content_hash.as_str(), fingerprint.to_string()],
            )
            .unwrap();
        }
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));

        LibraryScanner::scan_library(&state.db, &state);

        let conn = state.db.get().unwrap();
        let added_at: i64 = conn.query_row(include_str!("sql/scanner/scan_backfills_metadata_for_an_existing_placement_select.sql"), [content_hash.as_str()], |row| row.get(0)).unwrap();
        assert_eq!(added_at, 42, "metadata backfill must preserve the original addition time");
        let downloaded: bool = conn.query_row(include_str!("sql/scanner/scan_backfills_metadata_for_an_existing_placement_select_2.sql"), [content_hash.as_str()], |row| row.get(0)).unwrap();
        assert!(downloaded, "an existing file processed for metadata must still be marked present");
        drop(conn);

        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
    }

    /// Scanned files keep the name the user gave them, verbatim. Guards against
    /// anyone later "helpfully" normalizing scanned names to the download
    /// template: nothing about a filename is synchronized, and nothing on disk
    /// is ever renamed.
    /// Two physical folders copied with the same marker must both survive. The
    /// first one walked keeps the shared id and the duplicate receives a fresh
    /// identity, avoiding both aliasing and destructive cleanup of user data.
    #[test]
    fn a_folder_reusing_another_folders_marker_receives_a_fresh_identity() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        let library_root = library_root_path.canonicalize().unwrap();


        let shared_uuid = "6f1a3c52-2f4e-4a1b-9f77-0c3d5e8b21aa";
        let first = library_root_path.join("First");
        let second = library_root_path.join("Second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        fs::write(first.join(".biblos_uuid"), shared_uuid).unwrap();
        fs::write(second.join(".biblos_uuid"), shared_uuid).unwrap();

        let (state, hash_rx, _event_rx) = test_scanner_state(&library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));

        LibraryScanner::scan_library(&state.db, &state);

        let conn = state.db.get().unwrap();
        let directories = conn
            .prepare(include_str!("sql/scanner/a_folder_reusing_another_folders_marker_receives_a_fresh_identity_select.sql"))
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(directories.iter().map(|(_, name)| name.as_str()).collect::<Vec<_>>(), ["First", "Second"]);
        assert_eq!(directories.iter().filter(|(id, _)| id == shared_uuid).count(), 1, "exactly one folder keeps the copied identity");
        drop(conn);

        let first_marker = fs::read_to_string(first.join(".biblos_uuid")).unwrap();
        let second_marker = fs::read_to_string(second.join(".biblos_uuid")).unwrap();
        assert_ne!(first_marker, second_marker, "duplicate marker is replaced on one physical folder");
        assert!(first_marker == shared_uuid || second_marker == shared_uuid, "one physical folder keeps the copied identity");

        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
    }

    #[test]
    fn a_scanned_file_keeps_its_on_disk_name_and_renaming_it_emits_nothing() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        // Use a valid publication with stable embedded metadata. Invalid bytes
        // exercise metadata backfill and cannot isolate filename behavior.
        let mut epub = zip::ZipWriter::new(fs::File::create(library_root_path.join("my own name.epub")).unwrap());
        for (name, contents) in [
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", r#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
            (
                "package.opf",
                r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Stable title</dc:title><dc:description>Stable description</dc:description></metadata><manifest><item id="page" href="page.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="page"/></spine></package>"#,
            ),
            ("page.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>Publication</body></html>"#),
        ] {
            epub.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut epub, contents.as_bytes()).unwrap();
        }
        epub.finish().unwrap();
        let library_root = library_root_path.canonicalize().unwrap();

        let (state, hash_rx, _event_rx) = test_scanner_state(&library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));

        LibraryScanner::scan_library(&state.db, &state);

        let conn = state.db.get().unwrap();
        let stored: String = conn.query_row(include_str!("sql/scanner/a_scanned_file_keeps_its_on_disk_name_and_renaming_it_emits_nothing_select.sql"), (), |row| row.get(0)).unwrap();
        assert_eq!(stored, "my own name.epub", "the scanner records observed names verbatim");
        let assets = client_library::assets::live_book_hash_page(&conn, i64::MIN).unwrap();
        assert_eq!(assets.len(), 1);
        let hash = assets[0].1;
        assert!(!library_root.join(".bokheim/assets/book").join(hash.as_str()).exists(), "scanning must not duplicate the publication");
        assert_eq!(state.assets.read(crate::BlobKind::Book, &hash).unwrap().unwrap(), fs::read(library_root.join("my own name.epub")).unwrap());
        assert_eq!(client_library::thumbnail_jobs::thumbnail_work_state(&conn, &hash).unwrap().unwrap().0, "pending", "scanning persists cover work for maintenance");
        assert!(!state.assets.thumbnail_set_exists(&hash).unwrap(), "the scanner does not generate covers");
        let kinds_after_scan: Vec<String> = conn.prepare(include_str!("sql/scanner/a_scanned_file_keeps_its_on_disk_name_and_renaming_it_emits_nothing_select_2.sql")).unwrap().query_map((), |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
        drop(conn);

        // The user renames the file. The register that used to synchronize names
        // is gone, so this must not enqueue anything at all.
        fs::rename(library_root_path.join("my own name.epub"), library_root_path.join("renamed.epub")).unwrap();
        LibraryScanner::scan_library(&state.db, &state);

        let conn = state.db.get().unwrap();
        let kinds_after_rename: Vec<String> = conn.prepare(include_str!("sql/scanner/a_scanned_file_keeps_its_on_disk_name_and_renaming_it_emits_nothing_select_2.sql")).unwrap().query_map((), |row| row.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
        assert_eq!(kinds_after_rename, kinds_after_scan, "a rename on disk must not introduce a new outbox state kind");
        assert!(!kinds_after_rename.iter().any(|kind| kind.contains("file_name")), "no filename register may exist: {kinds_after_rename:?}");
        drop(conn);

        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
    }

    #[test]
    fn edited_m4b_remains_one_book_with_its_original_identity() {
        let app_data = tempdir().unwrap();
        let library_root = app_data.path().join("library");
        fs::create_dir_all(&library_root).unwrap();
        let path = library_root.join("book.m4b");
        let original = include_bytes!("../../tests/fixtures/embedded-cover.m4b");
        fs::write(&path, original).unwrap();
        // M4B identity is derived from encoded audio, not mutable file tags.
        let identity = publication_identity::identify(&mut fs::File::open(&path).unwrap()).unwrap();
        assert_ne!(identity, ContentHash::new(blake3::hash(original).to_hex().as_str()));

        let (state, hash_rx, _event_rx) = test_scanner_state(&library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));
        LibraryScanner::scan_library(&state.db, &state);
        assert_eq!(publication_identity::identify(&mut fs::File::open(&path).unwrap()).unwrap(), identity);
        client_library::assets::record_remote_asset(&*state.db.get().unwrap(), crate::BlobKind::Book, &identity).unwrap();
        {
            let mut conn = state.db.get().unwrap();
            let tx = conn.transaction().unwrap();
            let uploaded = client_library::assets::book_upload_intent(&tx, &identity).unwrap().unwrap();
            client_library::assets::complete_book_upload(&tx, &uploaded).unwrap();
            tx.commit().unwrap();
        }
        let mut duplicate = fs::read(&path).unwrap();
        duplicate.extend_from_slice(b"\0\0\0\x0cfreecopy");
        fs::write(library_root.join("duplicate.m4b"), duplicate).unwrap();
        LibraryScanner::scan_library(&state.db, &state);
        assert!(client_library::assets::book_upload_intent(&state.db.get().unwrap(), &identity).unwrap().is_none(), "an unselected duplicate must not alternate the queued revision");
        assert_eq!(state.db.get().unwrap().query_row(include_str!("sql/scanner/edited_m4b_remains_one_book_with_its_original_identity_select.sql"), [identity.as_str()], |row| row.get::<_, String>(0)).unwrap(), "book.m4b");
        state.db.get().unwrap().execute(include_str!("sql/scanner/edited_m4b_remains_one_book_with_its_original_identity_update.sql"),[]).unwrap();
        use std::io::Write as _;
        fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"\0\0\0\x0cfreedata").unwrap();
        LibraryScanner::scan_library(&state.db, &state);
        let conn = state.db.get().unwrap();
        let assets = client_library::assets::live_book_hash_page(&conn, i64::MIN).unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].1, identity);
        let request: String=conn.query_row(include_str!("sql/scanner/edited_m4b_remains_one_book_with_its_original_identity_select_2.sql"),[identity.as_str()],|row|row.get(0)).unwrap();
        assert!(serde_json::from_str::<metadata_contract::audible::LookupRequest>(&request).unwrap().duration_ms>0);
        assert_eq!(conn.query_row(include_str!("sql/scanner/edited_m4b_remains_one_book_with_its_original_identity_select_3.sql"),[],|row|row.get::<_,String>(0)).unwrap(),"no_match",
            "container-only edits must not repeat Audible lookup");
        assert!(client_library::assets::get_remote_asset_hashes(&conn, crate::BlobKind::Book).unwrap().contains(&identity), "local edits do not invalidate server availability");
        assert!(client_library::assets::book_upload_intent(&conn, &identity).unwrap().is_some(), "edited file must have explicit upload intent");
        assert_eq!(state.assets.read(crate::BlobKind::Book, &identity).unwrap().unwrap(), fs::read(&path).unwrap());
        conn.execute(include_str!("sql/scanner/edited_m4b_remains_one_book_with_its_original_identity_delete.sql"),[identity.as_str()]).unwrap();
        drop(conn);
        // An already indexed file must backfill missing recording evidence/jobs.
        LibraryScanner::scan_library(&state.db,&state);
        let conn=state.db.get().unwrap();
        assert_eq!(conn.query_row(include_str!("sql/scanner/edited_m4b_remains_one_book_with_its_original_identity_select_4.sql"),[],|row|row.get::<_,i64>(0)).unwrap(),1);
        conn.execute(include_str!("sql/scanner/edited_m4b_remains_one_book_with_its_original_identity_update.sql"),[]).unwrap();
        drop(conn);
        LibraryScanner::scan_library(&state.db,&state);
        let conn=state.db.get().unwrap();
        assert_eq!(conn.query_row(include_str!("sql/scanner/edited_m4b_remains_one_book_with_its_original_identity_select_3.sql"),[],|row|row.get::<_,String>(0)).unwrap(),"no_match");
        conn.execute(include_str!("sql/scanner/edited_m4b_remains_one_book_with_its_original_identity_update_3.sql"),[]).unwrap();
        drop(conn);
        LibraryScanner::scan_library(&state.db,&state);
        let conn=state.db.get().unwrap();
        assert_eq!(conn.query_row(include_str!("sql/scanner/edited_m4b_remains_one_book_with_its_original_identity_select_6.sql"),[],|row|row.get::<_,String>(0)).unwrap(),metadata_contract::audible::POLICY_VERSION);
        drop(conn);
        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
    }

    #[test]
    fn unreadable_pdfs_are_indexed_by_bytes_across_scan_batches() {
        let app_data = tempdir().unwrap();
        let library_root = app_data.path().join("library");
        fs::create_dir_all(&library_root).unwrap();
        // Unparseable PDFs still have byte identities and must be catalogued,
        // including when discovery crosses the bounded hash queue.
        for index in 0..(HASH_QUEUE_CAPACITY + 1) {
            fs::write(library_root.join(format!("broken-{index}.pdf")), format!("%PDF-1.4\nbroken {index}")).unwrap();
            fs::write(library_root.join(format!("book-{index}.epub")), scanner_epub(&format!("Book {index}"))).unwrap();
        }

        let (state, hash_rx, _event_rx) = test_scanner_state(&library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));
        LibraryScanner::scan_library(&state.db, &state);
        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
        let count: i64 = state.db.get().unwrap().query_row(include_str!("sql/scanner/unreadable_pdfs_are_indexed_by_bytes_across_scan_batches_select.sql"), [], |row| row.get(0)).unwrap();
        assert_eq!(count, (2 * (HASH_QUEUE_CAPACITY + 1)) as i64);
        assert!(state.hash_result_rx.is_empty());
    }

    #[test]
    fn large_scan_crosses_bounded_batches_and_repeat_scan_needs_no_hash_workers() {
        let app_data = tempdir().unwrap();
        let library_root_path = app_data.path().join("library");
        fs::create_dir_all(&library_root_path).unwrap();
        fs::create_dir_all(library_root_path.join("Shelf/Topic/Subtopic")).unwrap();
        for index in 0..(HASH_QUEUE_CAPACITY + 44) {
            fs::write(library_root_path.join(format!("book-{index:03}.epub")), scanner_epub(&format!("Book {index}"))).unwrap();
        }
        let library_root = library_root_path.canonicalize().unwrap();

        let (state, hash_rx, _event_rx) = test_scanner_state(&library_root);
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));

        LibraryScanner::scan_library(&state.db, &state);
        let conn = state.db.get().unwrap();
        let placements: i64 = conn.query_row(include_str!("sql/scanner/pending_folder_moves_do_not_block_unrelated_discovery_or_deletions_select_2.sql"), (), |row| row.get(0)).unwrap();
        assert_eq!(placements, (HASH_QUEUE_CAPACITY + 44) as i64);
        let first_outbox: i64 = conn.query_row(include_str!("sql/scanner/large_scan_crosses_bounded_batches_and_repeat_scan_needs_no_hash_workers_select_2.sql"), (), |row| row.get(0)).unwrap();
        conn.execute_batch(
            include_str!("sql/scanner/large_scan_crosses_bounded_batches_and_repeat_scan_needs_no_hash_workers_create.sql"),
        )
        .unwrap();
        drop(conn);

        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
        state.should_finish.store(false, Ordering::Release);

        LibraryScanner::scan_library(&state.db, &state);
        let conn = state.db.get().unwrap();
        let second_outbox: i64 = conn.query_row(include_str!("sql/scanner/large_scan_crosses_bounded_batches_and_repeat_scan_needs_no_hash_workers_select_2.sql"), (), |row| row.get(0)).unwrap();
        assert_eq!(second_outbox, first_outbox, "an unchanged rescan must use the fingerprint preload and emit no new state");
        let directory_writes: i64 = conn.query_row(include_str!("sql/scanner/large_scan_crosses_bounded_batches_and_repeat_scan_needs_no_hash_workers_select_4.sql"), (), |row| row.get(0)).unwrap();
        assert_eq!(directory_writes, 0, "an unchanged rescan must not rewrite known directories");
        assert!(state.hash_result_rx.is_empty());
    }
    #[test]
    fn missing_root_identity_and_unreadable_subtree_never_delete_placements() {
        let app_data = tempdir().unwrap();
        let root = app_data.path().join("library");
        fs::create_dir_all(&root).unwrap();

        let (state, _hash_rx, _events) = test_scanner_state(&root);
        let hash = fixture_content_hash(1);
        let conn = state.db.get().unwrap();
        conn.execute(include_str!("sql/scanner/scanner_with_missing_placement_insert.sql"), [hash.as_str()]).unwrap();
        conn.execute(
            include_str!("sql/scanner/missing_root_identity_and_unreadable_subtree_never_delete_placements_insert_2.sql"),
            [hash.as_str()],
        )
        .unwrap();
        client_library::sync_state::set_sync_metadata(&conn, "scan_seq", "10").unwrap();
        let marker = root.join(".bokheim/storage-identity");
        let identity = fs::read(&marker).unwrap();
        fs::write(&marker, b"different mounted drive").unwrap();
        LibraryScanner::scan_library(&state.db, &state);
        assert!(client_library::placements::library_trash(&conn).unwrap().books.is_empty());
        fs::write(&marker, identity).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/nonexistent-bokheim-drive", root.join("unavailable-drive")).unwrap();
            LibraryScanner::scan_library(&state.db, &state);
            assert!(client_library::placements::library_trash(&conn).unwrap().books.is_empty());
            fs::remove_file(root.join("unavailable-drive")).unwrap();
        }
        LibraryScanner::scan_library(&state.db, &state);
        assert_eq!(client_library::placements::library_trash(&conn).unwrap().books.len(), 1);
    }
    #[test]
    fn pending_folder_moves_do_not_block_unrelated_discovery_or_deletions() {
        let root = tempdir().unwrap();
        let library_root = root.path().join("library");
        fs::create_dir_all(library_root.join("Source")).unwrap();
        fs::write(library_root.join("Source/kept.epub"), scanner_epub("Kept")).unwrap();
        fs::write(library_root.join("vanished.epub"), scanner_epub("Vanished")).unwrap();

        let (state, hash_rx, _) = test_scanner_state(&library_root);
        let worker_state = state.clone();
        let worker = thread::spawn(move || LibraryScanner::hash_worker_loop(worker_state, hash_rx));
        LibraryScanner::scan_library(&state.db, &state);
        let mut conn = state.db.get().unwrap();
        let id = DirId::parse_str(&fs::read_to_string(library_root.join("Source/.biblos_uuid")).unwrap()).unwrap();
        let tx = client_library::schema::library_write_transaction(&mut conn).unwrap();
        client_library::placements::move_directory_record(&tx, &id, None, Some("Destination")).unwrap();
        tx.commit().unwrap();
        fs::write(library_root.join("Source/.biblos_uuid"), "blocked move").unwrap();
        fs::remove_file(library_root.join("vanished.epub")).unwrap();
        fs::write(library_root.join("new.epub"), scanner_epub("New")).unwrap();
        for _ in 0..3 {
            LibraryScanner::scan_library(&state.db, &state);
        }
        assert!(library_root.join("Source/kept.epub").exists(), "scan must not execute the queued move");
        assert!(!library_root.join("Destination").exists());
        assert_eq!(conn.query_row(include_str!("sql/scanner/pending_folder_moves_do_not_block_unrelated_discovery_or_deletions_select.sql"), [id.to_string()], |r| r.get::<_, String>(0)).unwrap(), "Destination");
        assert_eq!(conn.query_row(include_str!("sql/scanner/pending_folder_moves_do_not_block_unrelated_discovery_or_deletions_select_2.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), 2, "new file is discovered, pending book is protected, unrelated deletion is applied");
        assert_eq!(client_library::placements::library_trash(&conn).unwrap().books.len(), 1);
        assert!(client_library::file_work::has_pending_file_work(&conn).unwrap());
        assert!(!state.scan_active.load(Ordering::Acquire));
        state.should_finish.store(true, Ordering::Release);
        worker.join().unwrap();
    }

    #[test]
    fn commands_completed_during_discovery_still_invalidate_its_batch() {
        let root = tempdir().unwrap();
        let library_root = root.path().join("library");
        fs::create_dir_all(&library_root).unwrap();

        let (state, _, _) = test_scanner_state(&library_root);
        let mut conn = state.db.get().unwrap();
        let mut batch = ScanBatch { work_revision: Some(file_work_revision(&conn).unwrap()), ..ScanBatch::default() };
        let id = fixture_dir_id("stale-observation");
        batch.directories.push((id, crate::ROOT_DIR_ID, "Stale".into()));
        client_library::file_work::queue_directory_work(&conn, &id).unwrap();
        conn.execute(include_str!("sql/scanner/commands_completed_during_discovery_still_invalidate_its_batch_delete.sql"), []).unwrap();
        assert!(batch.commit(&mut conn, 1).is_err());
        assert!(!client_library::placements::directory_exists(&conn, &id.to_string()).unwrap());
    }
}
