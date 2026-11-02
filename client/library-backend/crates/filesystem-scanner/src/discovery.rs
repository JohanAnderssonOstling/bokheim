//! Book eligibility and tree discovery shared by explicit imports and scans.

use audiobook_archive::Track;
use book_model::BookFormat;
use library_files::filesystem::{DirectoryEntry, DirectoryWalk};
use std::fs;
use std::io;
use std::path::Path;

pub fn supported(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()).is_some_and(|name| BookFormat::from_file_name(name).is_some())
}

/// Tracks and sidecars must stay together until their folder is classified.
pub fn audiobook_member(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "mp3" | "nfo" | "cue"))
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| ["cover.jpg", "cover.jpeg", "cover.png", "cover.webp", "folder.jpg", "folder.jpeg", "folder.png", "folder.webp"].iter().any(|cover| name.eq_ignore_ascii_case(cover)))
}

/// Advance an existing walk past hidden entries and unsupported files.
/// Callers can prune returned folders before classifying their contents.
pub fn next_book_entry(walk: &mut DirectoryWalk) -> Option<io::Result<DirectoryEntry>> {
    while let Some(entry) = walk.next() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => return Some(Err(error)),
        };
        if entry.file_name().to_string_lossy().starts_with('.') {
            if entry.is_dir() {
                walk.skip_current_dir();
            }
        } else if entry.is_dir() || (entry.is_file() && supported(entry.path())) {
            return Some(Ok(entry));
        }
    }
    None
}

fn other_book_in_audiobook_candidate(folder: &Path, root: &Path, excluded: impl Fn(&Path) -> bool) -> io::Result<bool> {
    let mut children = Vec::new();
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if entry.path().strip_prefix(root).is_ok_and(&excluded) {
            continue;
        }
        if entry.file_type()?.is_file() && supported(&entry.path()) {
            return Ok(true);
        }
        if entry.file_type()?.is_dir() {
            let disc = entry.file_name().to_str().is_some_and(audiobook_folder::is_disc_folder_name);
            children.push((entry.path(), disc));
        }
    }
    while let Some((directory, direct_disc)) = children.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if entry.path().strip_prefix(root).is_ok_and(&excluded) {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                children.push((entry.path(), false));
            } else if kind.is_file() {
                let path = entry.path();
                let mp3 = path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("mp3"));
                if supported(&path) || (mp3 && !direct_disc) {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

/// Classify an audiobook once and retain its tracks for scan fingerprinting.
/// A mixed folder stays traversable even if unrelated audio sidecars are bad.
pub fn audiobook_tracks(folder: &Path, root: &Path, excluded: impl Fn(&Path) -> bool) -> io::Result<Option<Vec<Track>>> {
    let tracks = audiobook_folder::discover(folder);
    if tracks.as_ref().is_ok_and(|tracks| tracks.is_empty()) || other_book_in_audiobook_candidate(folder, root, excluded)? {
        return Ok(None);
    }
    tracks.map(Some)
}

pub fn is_audiobook_folder(folder: &Path) -> io::Result<bool> {
    Ok(audiobook_tracks(folder, folder, |_| false)?.is_some())
}
