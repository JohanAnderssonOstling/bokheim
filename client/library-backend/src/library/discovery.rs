//! Filesystem discovery capability owned by a library session.
//!
//! It contains only a root path. Database snapshots, persistence, scheduling,
//! events, and retry decisions belong to `LibrarySession`.

use crate::BackendError;
#[derive(Clone)]
pub(crate) struct LibraryScanner {
    pub(crate) root: String,
}

impl LibraryScanner {
    pub(crate) fn enabled(&self) -> bool {
        cfg!(all(feature = "scanner", not(target_arch = "wasm32")))
    }

    pub(crate) fn root(&self) -> &str {
        &self.root
    }

    pub(crate) fn discover(&self, snapshot: &library_database::ScanSnapshot) -> library_database::FilesystemScan {
        #[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
        {
            filesystem_scanner::filesystem_scan::discover(&self.root, snapshot)
        }
        #[cfg(not(all(feature = "scanner", not(target_arch = "wasm32"))))]
        {
            let _ = snapshot;
            library_database::FilesystemScan { readable: false, ..library_database::FilesystemScan::default() }
        }
    }

    pub(crate) async fn inspect(&self, scan: &library_database::FilesystemScan, snapshot: &library_database::ScanSnapshot, host: &std::sync::Arc<dyn cpu_host::CpuHost>) -> library_database::InspectedFilesystemScan {
        #[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
        {
            filesystem_scanner::filesystem_scan::inspect(&self.root, scan, snapshot, host).await
        }
        #[cfg(not(all(feature = "scanner", not(target_arch = "wasm32"))))]
        {
            let _ = (scan, snapshot, host);
            library_database::InspectedFilesystemScan { readable: false, ..library_database::InspectedFilesystemScan::default() }
        }
    }
}

pub fn estimate_folders(locators: Vec<String>, cancelled: &std::sync::atomic::AtomicBool) -> Result<(u64, u64, u64), BackendError> {
    #[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
    {
        count_books(locators, cancelled).ok_or_else(|| BackendError::message("Folder estimate cancelled"))
    }
    #[cfg(not(all(feature = "scanner", not(target_arch = "wasm32"))))]
    {
        let _ = (locators, cancelled);
        Err(BackendError::message("Folder size estimation is unavailable on this device"))
    }
}

/// Books, their total bytes and unreadable entries under `locators`, skipping
/// hidden entries and counting a file reached twice once. `None` once cancelled.
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
fn count_books(locators: Vec<String>, cancelled: &std::sync::atomic::AtomicBool) -> Option<(u64, u64, u64)> {
    use book_model::BookFormat;
    use std::collections::HashSet;
    let roots: Vec<_> = locators.into_iter().map(std::path::PathBuf::from).collect();
    let cancelled = || cancelled.load(std::sync::atomic::Ordering::Acquire);
    let (mut books, mut bytes, mut unreadable) = (0u64, 0u64, 0u64);
    let mut visited = HashSet::new();
    for root in roots {
        if cancelled() {
            return None;
        }
        let root = match root.canonicalize() {
            Ok(root) => root,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        let mut entries = library_files::filesystem::walk(root);
        loop {
            if cancelled() {
                return None;
            }
            let Some(entry) = entries.next() else { break };
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    unreadable += 1;
                    continue;
                }
            };
            if entry.file_name().to_string_lossy().starts_with('.') {
                if entry.is_dir() {
                    entries.skip_current_dir();
                }
                continue;
            }
            if !entry.is_file() || entry.file_name().to_str().and_then(BookFormat::from_file_name).is_none() || !visited.insert(entry.path().to_owned()) {
                continue;
            }
            match entry.length() {
                Some(length) if length > 0 => {
                    books += 1;
                    bytes = bytes.saturating_add(length);
                }
                Some(_) => {}
                None => unreadable += 1,
            }
        }
    }
    if cancelled() {
        None
    } else {
        Some((books, bytes, unreadable))
    }
}
