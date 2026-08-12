use crate::app::{read_library_manifest, LibraryEntry, LIBRARY_MANIFEST_FILENAME};
use crate::APP_HIDDEN_DIR;
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const KOBO_MARKER_DIRECTORY: &str = ".kobo";
const KOBO_LIBRARIES_PATH: &str = "Bokheim/Libraries";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KoboDevice {
    root: PathBuf,
    display_name: String,
}

impl KoboDevice {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn libraries_dir(&self) -> PathBuf {
        self.root.join(KOBO_LIBRARIES_PATH)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KoboTransferSummary {
    pub libraries: usize,
    pub copied_files: usize,
    pub unchanged_files: usize,
}

pub fn detect_kobo_devices() -> Vec<KoboDevice> {
    let mut candidates = BTreeSet::new();
    if let Some(path) = std::env::var_os("BOKHEIM_KOBO_MOUNT") {
        candidates.insert(PathBuf::from(path));
    }
    platform_mount_candidates(&mut candidates);

    candidates
        .into_iter()
        .filter_map(|root| {
            if !root.join(KOBO_MARKER_DIRECTORY).is_dir() {
                return None;
            }
            let root = root.canonicalize().ok()?;
            let volume = root.file_name().and_then(|name| name.to_str()).filter(|name| !name.is_empty()).unwrap_or("Kobo eReader");
            Some(KoboDevice { display_name: format!("Kobo eReader ({volume})"), root })
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn platform_mount_candidates(candidates: &mut BTreeSet<PathBuf>) {
    let Ok(mounts) = fs::read_to_string("/proc/self/mounts") else {
        return;
    };
    for line in mounts.lines() {
        if let Some(encoded) = line.split_whitespace().nth(1) {
            candidates.insert(PathBuf::from(decode_mount_field(encoded)));
        }
    }
}

#[cfg(target_os = "macos")]
fn platform_mount_candidates(candidates: &mut BTreeSet<PathBuf>) {
    if let Ok(volumes) = fs::read_dir("/Volumes") {
        candidates.extend(volumes.filter_map(Result::ok).map(|entry| entry.path()));
    }
}

#[cfg(target_os = "windows")]
fn platform_mount_candidates(candidates: &mut BTreeSet<PathBuf>) {
    for drive in b'A'..=b'Z' {
        candidates.insert(PathBuf::from(format!("{}:\\\\", drive as char)));
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_mount_candidates(_: &mut BTreeSet<PathBuf>) {}

#[cfg(target_os = "linux")]
fn decode_mount_field(value: &str) -> String {
    value.replace("\\040", " ").replace("\\011", "\t").replace("\\012", "\n").replace("\\134", "\\")
}

pub fn transfer_libraries_to_kobo(device: &KoboDevice, libraries: &[LibraryEntry]) -> io::Result<KoboTransferSummary> {
    let device_root = device.root.canonicalize()?;
    if !device_root.join(KOBO_MARKER_DIRECTORY).is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotFound, format!("Kobo is no longer mounted at {}", device.root.display())));
    }
    let libraries_dir = device_root.join(KOBO_LIBRARIES_PATH);
    fs::create_dir_all(&libraries_dir)?;
    let libraries_dir = libraries_dir.canonicalize()?;
    if !libraries_dir.starts_with(&device_root) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Kobo libraries directory escapes the mounted device"));
    }

    let mut summary = KoboTransferSummary::default();
    for library in libraries {
        transfer_library(library, &libraries_dir, &mut summary)?;
        summary.libraries += 1;
    }
    Ok(summary)
}

fn transfer_library(library: &LibraryEntry, libraries_dir: &Path, summary: &mut KoboTransferSummary) -> io::Result<()> {
    let source = library.library_path().canonicalize()?;
    let source_manifest = read_library_manifest(&source).map_err(invalid_data)?.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("library has no {LIBRARY_MANIFEST_FILENAME}: {}", source.display())))?;
    if &source_manifest.0 != library.library_id() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("library manifest identifies {}, but the registry identifies {}", source_manifest.0, library.library_id())));
    }

    let destination = libraries_dir.join(library.library_id().to_string());
    fs::create_dir_all(&destination)?;
    let destination = destination.canonicalize()?;
    if !destination.starts_with(libraries_dir) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "library destination escapes the Kobo libraries directory"));
    }
    if source == destination {
        return Ok(());
    }
    if let Some((destination_id, _)) = read_library_manifest(&destination).map_err(invalid_data)? {
        if destination_id != *library.library_id() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("destination {} belongs to library {}", destination.display(), destination_id)));
        }
    }

    copy_library_tree(&source, &destination, true, summary)?;
    copy_file_if_changed(&source.join(LIBRARY_MANIFEST_FILENAME), &destination.join(LIBRARY_MANIFEST_FILENAME), summary)?;
    Ok(())
}

fn copy_library_tree(source: &Path, destination: &Path, is_root: bool, summary: &mut KoboTransferSummary) -> io::Result<()> {
    for child in fs::read_dir(source)? {
        let child = child?;
        let name = child.file_name();
        if is_root && (name == APP_HIDDEN_DIR || name == LIBRARY_MANIFEST_FILENAME) {
            continue;
        }
        let file_type = child.file_type()?;
        if file_type.is_symlink() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("refusing to transfer symbolic link {}", child.path().display())));
        }
        let target = destination.join(&name);
        if file_type.is_dir() {
            fs::create_dir_all(&target)?;
            copy_library_tree(&child.path(), &target, false, summary)?;
        } else if file_type.is_file() {
            copy_file_if_changed(&child.path(), &target, summary)?;
        }
    }
    Ok(())
}

fn copy_file_if_changed(source: &Path, destination: &Path, summary: &mut KoboTransferSummary) -> io::Result<()> {
    if destination.is_file() && files_equal(source, destination)? {
        summary.unchanged_files += 1;
        return Ok(());
    }
    let parent = destination.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "transfer destination has no parent"))?;
    fs::create_dir_all(parent)?;
    let file_name = destination.file_name().and_then(|name| name.to_str()).unwrap_or("asset");
    let temporary = parent.join(format!(".{file_name}.bokheim-transfer-{}.tmp", Uuid::new_v4()));
    let backup = parent.join(format!(".{file_name}.bokheim-transfer-{}.bak", Uuid::new_v4()));
    let result = (|| {
        let mut input = File::open(source)?;
        let mut output = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        io::copy(&mut input, &mut output)?;
        output.flush()?;
        output.sync_all()?;
        drop(output);

        let had_destination = destination.exists();
        if had_destination {
            fs::rename(destination, &backup)?;
        }
        if let Err(error) = fs::rename(&temporary, destination) {
            if had_destination {
                let _ = fs::rename(&backup, destination);
            }
            return Err(error);
        }
        if had_destination {
            fs::remove_file(&backup)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
        if backup.exists() && !destination.exists() {
            let _ = fs::rename(&backup, destination);
        }
    }
    result?;
    summary.copied_files += 1;
    Ok(())
}

fn files_equal(left: &Path, right: &Path) -> io::Result<bool> {
    if fs::metadata(left)?.len() != fs::metadata(right)?.len() {
        return Ok(false);
    }
    let mut left = File::open(left)?;
    let mut right = File::open(right)?;
    let mut left_buffer = [0_u8; 64 * 1024];
    let mut right_buffer = [0_u8; 64 * 1024];
    loop {
        let left_read = left.read(&mut left_buffer)?;
        let right_read = right.read(&mut right_buffer)?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

fn invalid_data(error: Box<dyn std::error::Error>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
