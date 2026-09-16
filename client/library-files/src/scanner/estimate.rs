//! Fast pre-import size estimates using the same file eligibility as scanning.
use library_model::BookFormat;
use std::{collections::HashSet, path::PathBuf};
use walkdir::WalkDir;

/// Returns eligible files, physical bytes and unreadable entries. No file bodies
/// are read. Existing content and duplicate publications can reduce upload bytes.
pub fn estimate_folders(roots: Vec<PathBuf>) -> (u64, u64, u64) {
    estimate_folders_cancellable(roots, || false).expect("non-cancellable estimate")
}

/// Returns None when cancelled. Checks between entries; an in-progress OS call
/// must return before cancellation can be observed.
pub fn estimate_folders_cancellable(roots: Vec<PathBuf>, cancelled: impl Fn() -> bool) -> Option<(u64, u64, u64)> {
    let (mut books, mut bytes, mut unreadable) = (0u64, 0u64, 0u64);
    let mut visited = HashSet::new();
    for root in roots {
        if cancelled() { return None; }
        let root = match root.canonicalize() { Ok(root) => root, Err(_) => { unreadable += 1; continue; } };
        let mut entries = WalkDir::new(root).follow_links(false).min_depth(1).into_iter();
        loop {
            if cancelled() { return None; }
            let Some(entry) = entries.next() else { break };
            let entry = match entry { Ok(entry) => entry, Err(_) => { unreadable += 1; continue; } };
            if entry.file_name().to_string_lossy().starts_with('.') {
                if entry.file_type().is_dir() { entries.skip_current_dir(); }
                continue;
            }
            if !entry.file_type().is_file() || entry.path().extension().and_then(|ext| ext.to_str()).and_then(BookFormat::from_extension).is_none() || !visited.insert(entry.path().to_owned()) { continue; }
            match entry.metadata() {
                Ok(metadata) if metadata.len() > 0 => { books += 1; bytes = bytes.saturating_add(metadata.len()); }
                Ok(_) => {},
                Err(_) => unreadable += 1,
            }
        }
    }
    if cancelled() { None } else { Some((books, bytes, unreadable)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_stops_an_in_progress_walk_without_returning_a_partial_estimate() {
        let root = tempfile::tempdir().unwrap();
        for n in 0..20 { std::fs::write(root.path().join(format!("{n}.epub")), [1]).unwrap(); }
        let checks = std::cell::Cell::new(0);
        let estimate = estimate_folders_cancellable(vec![root.path().to_owned()], || {
            checks.set(checks.get() + 1);
            checks.get() == 5
        });
        assert_eq!(estimate, None);
        assert_eq!(checks.get(), 5);
        assert_eq!(estimate_folders(vec![root.path().to_owned()]), (20, 20, 0));
    }

    #[test]
    fn estimate_counts_eligible_files_without_following_links_or_duplicate_roots() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".hidden")).unwrap();
        for (name, bytes) in [("book.EPUB", 10), ("other.pdf", 20), ("empty.epub", 0), ("notes.txt", 5), (".hidden/hidden.epub", 40)] {
            std::fs::write(root.path().join(name), vec![0; bytes]).unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.path().join("book.EPUB"), root.path().join("link.epub")).unwrap();
        assert_eq!(estimate_folders(vec![root.path().to_owned(), root.path().to_owned()]), (2, 30, 0));
        assert_eq!(estimate_folders(vec![root.path().join("missing")]), (0, 0, 1));
    }
}
