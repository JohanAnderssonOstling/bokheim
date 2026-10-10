//! Immutable, approved snapshot selection. Hosts call activate once, before workers.
//! Native hosts select a prepared file; the browser, which bundles no taxonomy,
//! activates a prepared image it keeps in memory.
use rusqlite::Connection;
#[cfg(not(target_arch = "wasm32"))]
use rusqlite::OpenFlags;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::{path::Path, sync::OnceLock};

#[cfg(not(target_arch = "wasm32"))]
static ACTIVE: OnceLock<PathBuf> = OnceLock::new();
static ACTIVE_IMAGE: OnceLock<&'static [u8]> = OnceLock::new();

#[cfg(not(target_arch = "wasm32"))]
pub fn active_path() -> Option<&'static Path> {
    ACTIVE.get().map(PathBuf::as_path)
}

pub fn active_image() -> Option<&'static [u8]> {
    ACTIVE_IMAGE.get().copied()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn metadata(path: &Path) -> Result<(u64, u32), String> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
    metadata_in(&db)
}

fn metadata_in(db: &Connection) -> Result<(u64, u32), String> {
    let read = |key| -> Result<String, String> { db.query_row("SELECT value FROM taxonomy_meta WHERE key=?1", [key], |r| r.get(0)).map_err(|e| e.to_string()) };
    Ok((read("release_id")?.parse::<u64>().map_err(|e| e.to_string())?, read("format_version")?.parse::<u32>().map_err(|e| e.to_string())?))
}

/// Transform a private copy of a verified server snapshot into the client image.
/// Never called on the signed source or the currently active image.
#[cfg(not(target_arch = "wasm32"))]
pub fn prepare(path: &Path) -> Result<(), String> {
    let db = Connection::open(path).map_err(|e| e.to_string())?;
    prepare_in(&db)?;
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;").map_err(|e| e.to_string())?;
    Ok(())
}

fn prepare_in(db: &Connection) -> Result<(), String> {
    let concepts = crate::read_unified_taxonomy_connection(db, "taxonomy snapshot")?;
    let matcher = crate::UnifiedTaxonomy::from_concepts(concepts)?;
    db.execute_batch("DROP VIEW IF EXISTS unified_concept_paths").map_err(|e| e.to_string())?;
    db.execute_batch(include_str!("route_storage.sql")).map_err(|e| e.to_string())?;
    db.execute("INSERT INTO taxonomy_meta(key,value) VALUES('client_revision',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [matcher.revision_id()]).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn activate(path: &Path) -> Result<(), String> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    if let Some(previous) = ACTIVE.get() {
        return if previous == &path { Ok(()) } else { Err("taxonomy can only change at process startup".into()) };
    }
    let concepts = crate::read_unified_taxonomy_sqlite(&path)?;
    crate::install_unified_taxonomy_concepts(concepts)?;
    ACTIVE.set(path).map_err(|_| "taxonomy already selected".to_string())
}

/// The client image of a verified server snapshot, checked against the
/// release and format the signed manifest names. `source` records where the
/// snapshot came from, for workers that load it themselves.
pub fn prepare_image(snapshot: &[u8], expected: (u64, u32), source: &str) -> Result<Vec<u8>, String> {
    let db = open_snapshot(snapshot)?;
    if metadata_in(&db)? != expected {
        return Err("taxonomy version differs from signed manifest".into());
    }
    prepare_in(&db)?;
    db.execute("INSERT INTO taxonomy_meta(key,value) VALUES('source',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [source]).map_err(|e| e.to_string())?;
    Ok(db.serialize("main").map_err(|e| e.to_string())?.to_vec())
}

/// Installs a verified server snapshot's concepts for matching only, in a
/// worker that never attaches the taxonomy to a library.
pub fn install_snapshot(snapshot: &[u8]) -> Result<(), String> {
    let concepts = crate::read_unified_taxonomy_connection(&open_snapshot(snapshot)?, "taxonomy snapshot")?;
    crate::install_unified_taxonomy_concepts(concepts)
}

fn open_snapshot(snapshot: &[u8]) -> Result<Connection, String> {
    let mut snapshot = snapshot.to_vec();
    // A snapshot saved in WAL mode cannot be deserialized; mark it as a
    // rollback-journal database, which is what it is once in memory.
    if snapshot.len() > 19 && snapshot[18] == 2 {
        snapshot[18] = 1;
        snapshot[19] = 1;
    }
    let mut db = Connection::open_in_memory().map_err(|e| e.to_string())?;
    db.deserialize_read_exact("main", snapshot.as_slice(), snapshot.len(), false).map_err(|e| e.to_string())?;
    Ok(db)
}

/// Replaces the database at `path` with `image` in one transaction.
pub fn store_image(image: &[u8], path: &Path) -> Result<(), String> {
    let mut source = Connection::open_in_memory().map_err(|e| e.to_string())?;
    source.deserialize_read_exact("main", image, image.len(), true).map_err(|e| e.to_string())?;
    let mut destination = Connection::open(path).map_err(|e| e.to_string())?;
    let backup = rusqlite::backup::Backup::new(&source, &mut destination).map_err(|e| e.to_string())?;
    match backup.step(-1).map_err(|e| e.to_string())? {
        rusqlite::backup::StepResult::Done => Ok(()),
        _ => Err("taxonomy cache is busy".into()),
    }
}

/// A stored image with its release, format and source.
pub struct StoredImage {
    pub release: u64,
    pub format: u32,
    pub source: String,
    pub image: Vec<u8>,
}

pub fn load_image(path: &Path) -> Result<StoredImage, String> {
    let db = Connection::open(path).map_err(|e| e.to_string())?;
    let (release, format) = metadata_in(&db)?;
    let source = db.query_row("SELECT value FROM taxonomy_meta WHERE key='source'", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    Ok(StoredImage { release, format, source, image: db.serialize("main").map_err(|e| e.to_string())?.to_vec() })
}

/// The browser's `activate`: every library connection attaches this image.
pub fn activate_image(image: Vec<u8>) -> Result<(), String> {
    if ACTIVE_IMAGE.get().is_some() {
        return Err("taxonomy can only change at process startup".into());
    }
    let image: &'static [u8] = image.leak();
    let mut db = Connection::open_in_memory().map_err(|e| e.to_string())?;
    db.deserialize_bytes("main", image).map_err(|e| e.to_string())?;
    let concepts = crate::read_unified_taxonomy_connection(&db, "installed taxonomy image")?;
    crate::install_unified_taxonomy_concepts(concepts)?;
    ACTIVE_IMAGE.set(image).map_err(|_| "taxonomy already selected".to_string())
}
