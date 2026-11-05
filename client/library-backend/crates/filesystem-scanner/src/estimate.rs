//! Fast pre-import size estimates using the same file eligibility as scanning.

use book_model::BookFormat;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

/// Books, their total bytes and unreadable entries under `locators`, skipping
/// hidden entries and counting a file reached twice once. `None` once cancelled.
pub fn count_books(locators: Vec<String>, cancelled: &AtomicBool) -> Option<(u64, u64, u64)> {
    let cancelled = || cancelled.load(Ordering::Acquire);
    let (mut books, mut bytes, mut unreadable) = (0u64, 0u64, 0u64);
    let mut visited = HashSet::new();
    for root in locators.into_iter().map(PathBuf::from) {
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
