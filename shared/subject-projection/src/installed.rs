//! Immutable, approved snapshot selection. Hosts call activate once, before workers.
//! Every host selects an immutable file. SQLite pages and classification
//! matchers are loaded on demand using the same policy on every platform.
use crate::{runtime_format, sqlite_matcher::SqliteTaxonomy};
use operation_failure::OperationError;
use rusqlite::{Connection, OpenFlags};
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

static ACTIVE: OnceLock<PathBuf> = OnceLock::new();
pub const BUNDLED_RELEASE_ID: u64 = include!(concat!(env!("OUT_DIR"), "/taxonomy-release.rs"));
pub const PAGE_CACHE_KIB: i64 = 512;
#[cfg(not(target_arch = "wasm32"))]
pub const READ_ONLY_MMAP_BYTES: i64 = 16 * 1024 * 1024;

/// Identifies the prepared reference packaged with this application build.
pub fn bundled_source() -> String {
    format!("bundled:{}", include_str!(concat!(env!("OUT_DIR"), "/taxonomy.digest")))
}

pub fn active_path() -> Option<&'static Path> {
    ACTIVE.get().map(PathBuf::as_path)
}

pub fn path() -> Result<PathBuf, OperationError> {
    if let Some(path) = active_path() {
        return Ok(path.to_owned());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::bundled::path().map_err(Into::into)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Err("no taxonomy has been installed".into())
    }
}

/// Use the same read-only, immutable SQLite attachment on every host.
pub fn read_only_uri(path: &Path) -> Result<String, OperationError> {
    let path = path.to_str().ok_or("taxonomy path is not UTF-8")?;
    let mut encoded = String::with_capacity(path.len());
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    Ok(format!("file:{encoded}?mode=ro&immutable=1"))
}

pub fn open_read_only(path: &Path) -> Result<Connection, OperationError> {
    let db = Connection::open_with_flags(read_only_uri(path)?, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI).map_err(|error| OperationError::from_error(&error))?;
    db.pragma_update(None, "cache_size", -PAGE_CACHE_KIB).map_err(|error| OperationError::from_error(&error))?;
    #[cfg(not(target_arch = "wasm32"))]
    db.pragma_update(None, "mmap_size", READ_ONLY_MMAP_BYTES).map_err(|error| OperationError::from_error(&error))?;
    Ok(db)
}

pub fn metadata(path: &Path) -> Result<(u64, u32), OperationError> {
    let db = open_read_only(path)?;
    metadata_in(&db)
}

fn metadata_in(db: &Connection) -> Result<(u64, u32), OperationError> {
    let read = |key| -> Result<String, OperationError> { db.query_row("SELECT value FROM taxonomy_meta WHERE key=?1", [key], |r| r.get(0)).map_err(|error| OperationError::from_error(&error)) };
    Ok((read("release_id")?.parse::<u64>().map_err(|error| OperationError::from_error(&error))?, read("format_version")?.parse::<u32>().map_err(|error| OperationError::from_error(&error))?))
}

/// Verify a prepared image without modifying the bytes authenticated by the manifest.
pub fn validate(path: &Path, expected: (u64, u32)) -> Result<(), OperationError> {
    validate_in(&open_read_only(path)?, expected)
}

fn validate_in(db: &Connection, expected: (u64, u32)) -> Result<(), OperationError> {
    if metadata_in(db)? != expected {
        return Err("taxonomy version differs from signed manifest".into());
    }
    SqliteTaxonomy::validate(db)?;
    let headings: String = db.query_row("SELECT value FROM taxonomy_policy WHERE key='source_headings'", [], |r| r.get(0)).map_err(|e| OperationError::from_error(&e))?;
    crate::source_headings::validate_rules(&headings)?;
    let integrity: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(|e| OperationError::from_error(&e))?;
    if integrity != "ok" {
        return Err("taxonomy database failed its integrity check".into());
    }
    let invalid: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)", [], |r| r.get(0)).map_err(|e| OperationError::from_error(&e))?;
    if invalid {
        return Err("taxonomy contains invalid relationships".into());
    }
    Ok(())
}

pub fn activate(path: &Path) -> Result<(), OperationError> {
    #[cfg(not(target_arch = "wasm32"))]
    let path = path.canonicalize().map_err(|error| OperationError::from_error(&error))?;
    #[cfg(target_arch = "wasm32")]
    let path = path.to_owned();
    if let Some(previous) = ACTIVE.get() {
        return if previous == &path { Ok(()) } else { Err("taxonomy can only change at process startup".into()) };
    }
    SqliteTaxonomy::validate(&open_read_only(&path)?)?;
    ACTIVE.set(path).map_err(|_| OperationError::from("taxonomy already selected"))
}

pub fn validate_image(snapshot: &[u8], expected: (u64, u32)) -> Result<(), OperationError> {
    if !runtime_format::DOWNLOAD_FORMATS.contains(&expected.1) {
        return Err("taxonomy download requires a supported runtime format".into());
    }
    validate_in(&open_snapshot(snapshot)?, expected)
}

/// Installs an already prepared client image for matching in a worker.
pub fn install_snapshot(snapshot: &[u8]) -> Result<(), OperationError> {
    crate::runtime::install_from_connection(open_snapshot(snapshot)?).map_err(Into::into)
}

fn open_snapshot(snapshot: &[u8]) -> Result<Connection, OperationError> {
    let mut snapshot = snapshot.to_vec();
    // A snapshot saved in WAL mode cannot be deserialized; mark it as a
    // rollback-journal database, which is what it is once in memory.
    if snapshot.len() > 19 && snapshot[18] == 2 {
        snapshot[18] = 1;
        snapshot[19] = 1;
    }
    let mut db = Connection::open_in_memory().map_err(|error| OperationError::from_error(&error))?;
    db.deserialize_read_exact("main", snapshot.as_slice(), snapshot.len(), false).map_err(|error| OperationError::from_error(&error))?;
    Ok(db)
}

/// The selected content release, independent of its storage format or projection policy.
pub fn selected_release() -> Result<u64, OperationError> {
    Ok(metadata(&path()?)?.0)
}
